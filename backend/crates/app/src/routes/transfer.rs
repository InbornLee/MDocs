//! Document import / export: single files, ZIP archives, directories
//! (multi-file multipart) and tree JSON — all funnelled through one import
//! core that produces a single commit and refreshes the link graph.

use std::collections::{BTreeMap, HashMap};

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{document, doc_version};
use crate::error::{AppError, AppResult};
use crate::services::{
    attachment, auth::CurrentUser, links, links::LinkUpdate, merge, perm, perm::Role, vcs,
};
use crate::state::AppState;

use super::document::BranchQuery;

const MAX_IMPORT_BODY: usize = 128 * 1024 * 1024; // 128 MB total multipart body
const MAX_DOC_BYTES: usize = 16 * 1024 * 1024; // 16 MB per document

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects/{slug}/import", post(import_upload))
        .route("/api/projects/{slug}/import/tree", post(import_tree))
        .route("/api/projects/{slug}/export/zip", get(export_zip))
        .route("/api/projects/{slug}/export/tree", get(export_tree))
        .route("/api/projects/{slug}/docs/{doc_id}/export", get(export_doc))
        .layer(DefaultBodyLimit::max(MAX_IMPORT_BODY))
}

/// Attachment bytes are served without authentication on purpose: a browser
/// `<img src>` cannot attach the `Authorization` header, and the public reader
/// has to render them too. The project id in the URL is a UUID, so the path is
/// not guessable.
pub fn public_routes() -> Router<AppState> {
    Router::new().route("/uploads/attachments/{project_id}/{*path}", get(get_attachment))
}

async fn get_attachment(
    State(state): State<AppState>,
    Path((project_id, path)): Path<(String, String)>,
) -> AppResult<Response> {
    let project_id = Uuid::parse_str(&project_id)
        .map_err(|_| AppError::NotFound("attachment not found".into()))?;
    let bytes = attachment::read(&state, project_id, &path).await?;
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref())
            .map_err(|_| AppError::Internal("bad content type".into()))?,
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    Ok((StatusCode::OK, headers, Body::from(bytes)).into_response_ext())
}

// ---------- shared import core ----------

struct ImportItem {
    path: String,
    content: String,
}

/// A non-markdown file (image and friends) shipped with an import.
struct AssetItem {
    path: String,
    bytes: Vec<u8>,
}

#[derive(Serialize)]
struct SkippedItem {
    path: String,
    reason: String,
}

/// An incoming file that already exists in the project: either the exact same
/// path, or the same file name under another directory.
#[derive(Serialize, Clone)]
struct ConflictItem {
    /// path as found in the upload
    path: String,
    /// "path" (same path) | "name" (same file name, other directory)
    kind: &'static str,
    existing_doc_id: Uuid,
    existing_path: String,
    existing_title: String,
}

#[derive(Serialize)]
struct MergedItem {
    path: String,
    has_conflict: bool,
}

#[derive(Serialize)]
struct ImportResultDto {
    branch: String,
    imported: usize,
    /// attachments (images and other non-markdown files) written by this import
    assets: usize,
    skipped: Vec<SkippedItem>,
    commit_id: Option<Uuid>,
    /// files whose name already exists in the project (mode `check` reports
    /// them, the applying modes report what they were applied to)
    conflicts: Vec<ConflictItem>,
    /// documents written through a three-way merge, flagged when the merge left
    /// diff3 conflict markers behind
    merged: Vec<MergedItem>,
    /// true for a dry run (`mode = "check"`), which writes nothing
    checked: bool,
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn dir_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

fn current_version(states: &HashMap<Uuid, vcs::DocState>, doc_id: Uuid) -> Option<Uuid> {
    states.get(&doc_id).and_then(|s| s.version_id)
}

/// Content of the version just before `current`: the merge base used when an
/// import is merged into an existing document. `None` when there is no history.
async fn previous_content(
    db: &sea_orm::DatabaseConnection,
    doc_id: Uuid,
    current: Option<Uuid>,
) -> Result<Option<String>, sea_orm::DbErr> {
    let Some(current) = current else {
        return Ok(None);
    };
    let versions = doc_version::Entity::find()
        .filter(doc_version::Column::DocumentId.eq(doc_id))
        .order_by_desc(doc_version::Column::CreatedAt)
        .all(db)
        .await?;
    Ok(versions
        .iter()
        .position(|v| v.id == current)
        .and_then(|i| versions.get(i + 1))
        .map(|v| v.content.clone()))
}

/// Apply a batch of documents to a branch as ONE commit, then refresh links.
/// `mode` decides what happens to incoming files that already exist in the
/// project (same path, or same file name in another directory): `check` only
/// reports them, `skip` (default) leaves the existing document untouched,
/// `overwrite` replaces its content and `merge` three-way merges into it.
/// `assets` are the non-markdown files of the upload; they are not versioned,
/// they are written straight to the project's attachment store.
async fn apply_import_as(
    state: &AppState,
    cur: &CurrentUser,
    proj: &crate::entities::project::Model,
    branch: Option<&str>,
    mode: &str,
    message: Option<&str>,
    items: Vec<ImportItem>,
    assets: Vec<AssetItem>,
) -> AppResult<ImportResultDto> {
    perm::require_role(&state.db, proj, cur.id, Role::Editor).await?;
    let br = vcs::resolve_branch(&state.db, proj, branch).await?;

    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let states = vcs::branch_doc_states(&state.db, &graph, br.head_commit_id).await?;

    // live documents of this branch, indexed by id / path / file name
    let alive_ids: Vec<Uuid> = states
        .values()
        .filter(|s| s.change_type != "deleted")
        .map(|s| s.document_id)
        .collect();
    let alive_docs = if alive_ids.is_empty() {
        vec![]
    } else {
        document::Entity::find()
            .filter(document::Column::ProjectId.eq(proj.id))
            .filter(document::Column::Id.is_in(alive_ids))
            .all(&state.db)
            .await?
    };
    let mut by_id: HashMap<Uuid, document::Model> = HashMap::new();
    let mut by_path: HashMap<String, Uuid> = HashMap::new();
    let mut by_name: HashMap<String, Vec<Uuid>> = HashMap::new();
    for d in alive_docs {
        by_name
            .entry(file_name(&d.path).to_string())
            .or_default()
            .push(d.id);
        by_path.insert(d.path.clone(), d.id);
        by_id.insert(d.id, d);
    }

    // dedupe within the batch: later entries win
    let mut batch: BTreeMap<String, String> = BTreeMap::new();
    let mut skipped: Vec<SkippedItem> = Vec::new();
    for it in items {
        let path = it.path.trim().trim_start_matches("./").to_string();
        if let Err(e) = vcs::validate_doc_path(&path) {
            skipped.push(SkippedItem {
                path: it.path,
                reason: e.to_string(),
            });
            continue;
        }
        if it.content.len() > MAX_DOC_BYTES {
            skipped.push(SkippedItem {
                path,
                reason: "file larger than 16MB".into(),
            });
            continue;
        }
        batch.insert(path, it.content);
    }

    // match every incoming file against the live documents: the same path is a
    // direct clash, the same file name in another directory is almost always the
    // same document imported a second time
    let mut conflicts: Vec<ConflictItem> = Vec::new();
    let mut target_of: HashMap<String, Uuid> = HashMap::new();
    for path in batch.keys() {
        if let Some(&id) = by_path.get(path.as_str()) {
            let d = &by_id[&id];
            conflicts.push(ConflictItem {
                path: path.clone(),
                kind: "path",
                existing_doc_id: id,
                existing_path: d.path.clone(),
                existing_title: d.title.clone(),
            });
            target_of.insert(path.clone(), id);
            continue;
        }
        let Some(ids) = by_name.get(file_name(path)) else {
            continue;
        };
        let dir = dir_of(path);
        // same directory first, then a stable path order
        let picked = ids
            .iter()
            .copied()
            .min_by_key(|id| {
                let d = &by_id[id];
                (dir.is_empty() || dir_of(&d.path) != dir, d.path.clone())
            })
            .expect("by_name entries are never empty");
        let d = &by_id[&picked];
        conflicts.push(ConflictItem {
            path: path.clone(),
            kind: "name",
            existing_doc_id: picked,
            existing_path: d.path.clone(),
            existing_title: d.title.clone(),
        });
        target_of.insert(path.clone(), picked);
    }

    // dry run: report the clashes and write nothing
    if mode == "check" {
        return Ok(ImportResultDto {
            branch: br.name,
            imported: 0,
            assets: 0,
            skipped,
            commit_id: None,
            conflicts,
            merged: Vec::new(),
            checked: true,
        });
    }

    // attachments are not versioned and cannot conflict: they are written
    // straight to the attachment store, mirroring their path in the project
    let mut assets_stored = 0usize;
    for a in assets {
        match attachment::store(state, proj.id, &a.path, &a.bytes).await {
            Ok(()) => assets_stored += 1,
            Err(e) => skipped.push(SkippedItem {
                path: a.path,
                reason: e.to_string(),
            }),
        }
    }

    // files without a counterpart become new documents; clashing files are
    // skipped or written into the existing document (overwrite / merge), so an
    // import never creates a second copy of a document that is already there
    let overwrite = mode == "overwrite";
    let merge_into_existing = mode == "merge";
    let mut new_items: Vec<(String, String)> = Vec::new();
    // existing document -> (its path, resolved content, merge conflicted)
    let mut rewrites: BTreeMap<Uuid, (String, String, bool)> = BTreeMap::new();
    for (path, content) in batch {
        let Some(&target) = target_of.get(&path) else {
            new_items.push((path, content));
            continue;
        };
        if !overwrite && !merge_into_existing {
            skipped.push(SkippedItem {
                path,
                reason: "a document with the same name already exists in this project".into(),
            });
            continue;
        }
        let existing_path = by_id[&target].path.clone();
        let base = previous_content(&state.db, target, current_version(&states, target)).await?;
        if let Some((_, current, _)) = rewrites.get(&target) {
            // a second incoming file resolving to the same document: fold it in
            // so the commit keeps exactly one change per document
            if overwrite {
                rewrites.insert(target, (existing_path, content, false));
            } else {
                let base = base.unwrap_or_else(|| current.clone());
                let out = merge::merge_file(&base, current, &content);
                rewrites.insert(target, (existing_path, out.content, out.has_conflict));
            }
            continue;
        }
        let ours = match current_version(&states, target) {
            Some(vid) => doc_version::Entity::find_by_id(vid)
                .one(&state.db)
                .await?
                .map(|v| v.content)
                .unwrap_or_default(),
            None => String::new(),
        };
        if overwrite {
            rewrites.insert(target, (existing_path, content, false));
        } else {
            // three-way merge so edits made on both sides survive; a document
            // without history has nothing to merge against and is replaced
            let base = base.unwrap_or_else(|| ours.clone());
            let out = merge::merge_file(&base, &ours, &content);
            rewrites.insert(target, (existing_path, out.content, out.has_conflict));
        }
    }

    if new_items.is_empty() && rewrites.is_empty() && assets_stored == 0 {
        return Ok(ImportResultDto {
            branch: br.name,
            imported: 0,
            assets: 0,
            skipped,
            commit_id: None,
            conflicts,
            merged: Vec::new(),
            checked: false,
        });
    }

    let now = Utc::now();
    let txn = state.db.begin().await?;
    let mut changes: Vec<vcs::Change> = Vec::with_capacity(new_items.len() + rewrites.len());
    let mut link_updates: Vec<LinkUpdate> = Vec::with_capacity(new_items.len() + rewrites.len());
    let mut merged: Vec<MergedItem> = Vec::new();
    let mut imported = 0usize;

    for (path, content) in &new_items {
        // reuse a (previously deleted) row or insert a fresh one
        let doc = match document::Entity::find()
            .filter(document::Column::ProjectId.eq(proj.id))
            .filter(document::Column::Path.eq(path))
            .one(&txn)
            .await?
        {
            Some(existing) => {
                let mut am = existing.into_active_model();
                am.title = Set(vcs::extract_title(content, path));
                am.updated_at = Set(now);
                am.update(&txn).await?
            }
            None => document::ActiveModel {
                id: Set(Uuid::now_v7()),
                project_id: Set(proj.id),
                path: Set(path.clone()),
                title: Set(vcs::extract_title(content, path)),
                published: Set(true),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&txn)
            .await?,
        };
        let ver = vcs::create_version(&txn, doc.id, content, cur.id).await?;
        changes.push(vcs::Change {
            document_id: doc.id,
            change_type: "added",
            version_id: Some(ver.id),
            old_path: None,
        });
        link_updates.push(LinkUpdate {
            doc_id: doc.id,
            path,
            content: Some(content),
        });
        imported += 1;
    }

    for (target, (path, content, has_conflict)) in &rewrites {
        let mut am: document::ActiveModel = by_id[target].clone().into_active_model();
        am.title = Set(vcs::extract_title(content, path));
        am.updated_at = Set(now);
        am.update(&txn).await?;
        let ver = vcs::create_version(&txn, *target, content, cur.id).await?;
        changes.push(vcs::Change {
            document_id: *target,
            change_type: "modified",
            version_id: Some(ver.id),
            old_path: None,
        });
        link_updates.push(LinkUpdate {
            doc_id: *target,
            path,
            content: Some(content),
        });
        merged.push(MergedItem {
            path: path.clone(),
            has_conflict: *has_conflict,
        });
        imported += 1;
    }

    let msg = match message {
        Some(m) if !m.trim().is_empty() => m.trim().to_string(),
        _ => format!("import {imported} documents"),
    };
    let commit = vcs::create_commit(
        &txn,
        proj.id,
        br.id,
        br.head_commit_id,
        cur.id,
        &msg,
        "import",
        changes,
    )
    .await?;

    links::after_commit(
        &txn,
        proj.id,
        br.id,
        Some(commit.id),
        &link_updates,
    )
    .await?;
    txn.commit().await?;

    Ok(ImportResultDto {
        branch: br.name,
        imported,
        assets: assets_stored,
        skipped,
        commit_id: Some(commit.id),
        conflicts,
        merged,
        checked: false,
    })
}

// ---------- multipart import (files / directory / zip) ----------

async fn import_upload(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
    mut mp: Multipart,
) -> AppResult<Json<ImportResultDto>> {
    let proj = perm::get_project(&state.db, &slug).await?;

    let mut branch: Option<String> = None;
    let mut mode = String::from("skip");
    let mut message: Option<String> = None;
    let mut paths_json: Option<String> = None;
    // (filename, bytes) in arrival order
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();

    while let Some(field) = mp
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("multipart error: {e}")))?
    {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "branch" => branch = Some(field.text().await.unwrap_or_default()),
            "mode" => mode = field.text().await.unwrap_or_default(),
            "message" => message = Some(field.text().await.unwrap_or_default()),
            "paths" => paths_json = Some(field.text().await.unwrap_or_default()),
            _ => {
                let filename = field
                    .file_name()
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                // `Field::chunk()` returns only the *next* fragment of the field
                // body, so using it here silently truncates anything larger than
                // one chunk (which made bigger ZIPs fail with "Could not find
                // EOCD"). Read the whole field instead.
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("upload error: {e}")))?;
                files.push((filename, data.to_vec()));
            }
        }
    }

    if files.is_empty() {
        return Err(AppError::BadRequest("no files uploaded".into()));
    }

    // treat empty-string branch / message as absent
    let branch = branch.filter(|b| !b.trim().is_empty());
    let message = message.filter(|m| !m.trim().is_empty());

    let paths: Vec<String> = paths_json
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default();

    let mut items: Vec<ImportItem> = Vec::new();
    let mut assets: Vec<AssetItem> = Vec::new();
    for (i, (filename, bytes)) in files.into_iter().enumerate() {
        let lower = filename.to_lowercase();
        if lower.ends_with(".zip") {
            expand_zip(&filename, &bytes, &mut items, &mut assets)?;
            continue;
        }
        // plain file: take the declared relative path, else the filename
        let path = paths
            .get(i)
            .cloned()
            .filter(|p| !p.trim().is_empty())
            .unwrap_or_else(|| filename.clone());
        if is_markdown(&path) {
            match String::from_utf8(bytes) {
                Ok(content) => items.push(ImportItem { path, content }),
                Err(_) => {
                    return Err(AppError::BadRequest(format!(
                        "'{path}' is not valid UTF-8 markdown"
                    )))
                }
            }
        } else {
            // images and any other non-markdown file ride along as attachments
            assets.push(AssetItem { path, bytes });
        }
    }

    let res = apply_import_as(
        &state,
        &cur,
        &proj,
        branch.as_deref(),
        &mode,
        message.as_deref(),
        items,
        assets,
    )
    .await?;
    Ok(Json(res))
}

fn is_markdown(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

/// Decode a zip entry name from its raw bytes.
///
/// `ZipFile::name()` falls back to CP437 whenever the archive does not set the
/// UTF-8 name flag, but Chinese Windows archivers write GBK instead, which
/// turns every non-ASCII name into mojibake. The raw bytes are decoded here, so
/// UTF-8 archives stay untouched and GBK ones come back readable.
fn decode_zip_name(raw: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(raw) {
        return s.to_string();
    }
    let (decoded, _, had_errors) = encoding_rs::GBK.decode(raw);
    if !had_errors {
        return decoded.into_owned();
    }
    String::from_utf8_lossy(raw).into_owned()
}

fn expand_zip(
    name: &str,
    bytes: &[u8],
    docs: &mut Vec<ImportItem>,
    assets: &mut Vec<AssetItem>,
) -> AppResult<()> {
    if bytes.is_empty() {
        return Err(AppError::BadRequest(format!(
            "'{name}' is empty (0 bytes received)"
        )));
    }
    let rdr = std::io::Cursor::new(bytes);
    // The received size is included on purpose: a truncated upload is the most
    // common cause of a missing end-of-central-directory record.
    let mut ar = zip::ZipArchive::new(rdr).map_err(|e| {
        AppError::BadRequest(format!(
            "'{name}': cannot read zip archive ({} bytes received): {e}",
            bytes.len()
        ))
    })?;
    for i in 0..ar.len() {
        let mut f = ar
            .by_index(i)
            .map_err(|e| AppError::BadRequest(format!("zip read error: {e}")))?;
        if f.is_dir() {
            continue;
        }
        let raw = decode_zip_name(f.name_raw()).replace('\\', "/");
        let mut buf = Vec::with_capacity(f.size() as usize);
        std::io::Read::read_to_end(&mut f, &mut buf)
            .map_err(|e| AppError::BadRequest(format!("zip extract error: {e}")))?;
        if is_markdown(&raw) {
            let content = String::from_utf8(buf).map_err(|_| {
                AppError::BadRequest(format!("zip entry '{raw}' is not valid UTF-8 markdown"))
            })?;
            docs.push(ImportItem { path: raw, content });
        } else {
            // images (and anything else) become attachments, keeping their path
            assets.push(AssetItem { path: raw, bytes: buf });
        }
    }
    Ok(())
}

// ---------- tree JSON import ----------

#[derive(Deserialize)]
struct TreeDocReq {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct TreeImportReq {
    branch: Option<String>,
    mode: Option<String>,
    message: Option<String>,
    docs: Vec<TreeDocReq>,
}

async fn import_tree(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
    Json(req): Json<TreeImportReq>,
) -> AppResult<Json<ImportResultDto>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    if req.docs.is_empty() {
        return Err(AppError::BadRequest("tree contains no documents".into()));
    }
    if req.docs.len() > 5000 {
        return Err(AppError::BadRequest("tree too large (max 5000 docs)".into()));
    }
    let items = req
        .docs
        .into_iter()
        .map(|d| ImportItem {
            path: d.path,
            content: d.content,
        })
        .collect();
    let mode = req.mode.as_deref().unwrap_or("skip");
    let branch = req.branch.filter(|b| !b.trim().is_empty());
    let res = apply_import_as(
        &state,
        &cur,
        &proj,
        branch.as_deref(),
        mode,
        req.message.as_deref(),
        items,
        Vec::new(),
    )
    .await?;
    Ok(Json(res))
}

// ---------- export helpers ----------

/// Alive (path, content, title) docs of a branch, ordered by path.
async fn collect_alive(
    db: &impl ConnectionTrait,
    proj: &crate::entities::project::Model,
    br: &crate::entities::branch::Model,
) -> AppResult<Vec<(String, String, String)>> {
    let graph = vcs::CommitGraph::load(db, proj.id).await?;
    let states = vcs::branch_doc_states(db, &graph, br.head_commit_id).await?;
    let alive: Vec<(Uuid, Option<Uuid>)> = states
        .values()
        .filter(|s| s.change_type != "deleted")
        .map(|s| (s.document_id, s.version_id))
        .collect();
    if alive.is_empty() {
        return Ok(vec![]);
    }
    let ids: Vec<Uuid> = alive.iter().map(|(id, _)| *id).collect();
    let docs = document::Entity::find()
        .filter(document::Column::ProjectId.eq(proj.id))
        .filter(document::Column::Id.is_in(ids))
        .all(db)
        .await?;
    let ver_ids: Vec<Uuid> = alive.iter().filter_map(|(_, v)| *v).collect();
    let versions: std::collections::HashMap<Uuid, String> = if ver_ids.is_empty() {
        Default::default()
    } else {
        doc_version::Entity::find()
            .filter(doc_version::Column::Id.is_in(ver_ids))
            .all(db)
            .await?
            .into_iter()
            .map(|v| (v.id, v.content))
            .collect()
    };
    let title_by_id: std::collections::HashMap<Uuid, String> =
        docs.iter().map(|d| (d.id, d.title.clone())).collect();
    let path_by_id: std::collections::HashMap<Uuid, String> =
        docs.iter().map(|d| (d.id, d.path.clone())).collect();

    let mut out: Vec<(String, String, String)> = alive
        .into_iter()
        .filter_map(|(id, vid)| {
            let content = vid.and_then(|v| versions.get(&v).cloned())?;
            Some((
                path_by_id.get(&id)?.clone(),
                content,
                title_by_id.get(&id)?.clone(),
            ))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn download_response(
    bytes: Vec<u8>,
    content_type: &str,
    filename: &str,
) -> AppResult<Response> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type)
            .map_err(|_| AppError::Internal("bad content type".into()))?,
    );
    // RFC 5987: ascii fallback + UTF-8 encoded filename
    let ascii: String = filename
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || " ._-()".contains(c) { c } else { '_' })
        .collect();
    let utf8 = percent_encoding::utf8_percent_encode(
        filename,
        percent_encoding::NON_ALPHANUMERIC,
    )
    .to_string();
    let cd = format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{utf8}");
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&cd)
            .map_err(|_| AppError::Internal("bad disposition".into()))?,
    );
    Ok((StatusCode::OK, headers, Body::from(bytes)).into_response_ext())
}

trait IntoResponseExt {
    fn into_response_ext(self) -> Response;
}

impl IntoResponseExt for (StatusCode, HeaderMap, Body) {
    fn into_response_ext(self) -> Response {
        let (status, headers, body) = self;
        let mut res = Response::new(body);
        *res.status_mut() = status;
        *res.headers_mut() = headers;
        res
    }
}

// ---------- export handlers ----------

async fn export_zip(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
    Query(q): Query<BranchQuery>,
) -> AppResult<Response> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Public).await?;
    let br = vcs::resolve_branch(&state.db, &proj, q.branch.as_deref()).await?;
    let docs = collect_alive(&state.db, &proj, &br).await?;
    if docs.is_empty() {
        return Err(AppError::NotFound("no documents on this branch".into()));
    }

    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (path, content, _title) in &docs {
        w.start_file(path, opts)
            .map_err(|e| AppError::Internal(format!("zip write error: {e}")))?;
        std::io::Write::write_all(&mut w, content.as_bytes())
            .map_err(|e| AppError::Internal(format!("zip write error: {e}")))?;
    }
    // attachments travel along, mirroring the path they were imported with so
    // the markdown references stay valid inside the archive
    for rel in attachment::list_all(&state, proj.id).await? {
        let Ok(bytes) = attachment::read(&state, proj.id, &rel).await else {
            continue;
        };
        w.start_file(&rel, opts)
            .map_err(|e| AppError::Internal(format!("zip write error: {e}")))?;
        std::io::Write::write_all(&mut w, &bytes)
            .map_err(|e| AppError::Internal(format!("zip write error: {e}")))?;
    }
    let bytes = w
        .finish()
        .map_err(|e| AppError::Internal(format!("zip finish error: {e}")))?
        .into_inner();

    let filename = format!("{}-{}.zip", proj.slug, br.name);
    download_response(bytes, "application/zip", &filename)
}

async fn export_doc(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path((slug, doc_id)): Path<(String, Uuid)>,
    Query(q): Query<BranchQuery>,
) -> AppResult<Response> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Public).await?;
    let br = vcs::resolve_branch(&state.db, &proj, q.branch.as_deref()).await?;

    let doc = document::Entity::find()
        .filter(document::Column::ProjectId.eq(proj.id))
        .filter(document::Column::Id.eq(doc_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("document not found".into()))?;

    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let st = vcs::doc_state(&state.db, &graph, br.head_commit_id, doc.id)
        .await?
        .filter(|s| s.change_type != "deleted")
        .ok_or_else(|| AppError::NotFound("document not on this branch".into()))?;
    let Some(vid) = st.version_id else {
        return Err(AppError::NotFound("document has no content".into()));
    };
    let ver = doc_version::Entity::find_by_id(vid)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::Internal("version missing".into()))?;

    download_response(
        ver.content.into_bytes(),
        "text/markdown; charset=utf-8",
        &doc.path,
    )
}

#[derive(Serialize)]
struct TreeExportDoc {
    path: String,
    title: String,
    content: String,
}

#[derive(Serialize)]
struct TreeExportDto {
    version: u32,
    project_slug: String,
    project_name: String,
    branch: String,
    exported_at: chrono::DateTime<Utc>,
    docs: Vec<TreeExportDoc>,
}

async fn export_tree(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
    Query(q): Query<BranchQuery>,
) -> AppResult<Response> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Public).await?;
    let br = vcs::resolve_branch(&state.db, &proj, q.branch.as_deref()).await?;
    let docs = collect_alive(&state.db, &proj, &br).await?;

    let dto = TreeExportDto {
        version: 1,
        project_slug: proj.slug.clone(),
        project_name: proj.name.clone(),
        branch: br.name.clone(),
        exported_at: Utc::now(),
        docs: docs
            .into_iter()
            .map(|(path, content, title)| TreeExportDoc { path, title, content })
            .collect(),
    };
    let bytes = serde_json::to_vec_pretty(&dto)
        .map_err(|e| AppError::Internal(format!("json error: {e}")))?;
    let filename = format!("{}-{}-tree.json", proj.slug, br.name);
    download_response(bytes, "application/json; charset=utf-8", &filename)
}
