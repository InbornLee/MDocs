//! Document CRUD, content reads, and optimistic-locked saves.

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{commit, commit_doc, document, doc_version, link_edge, user};
use crate::error::{AppError, AppResult};
use crate::services::{
    auth::CurrentUser, diff, links, links::LinkUpdate, perm, perm::Role, search, vcs,
};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/projects/{slug}/docs",
            get(list_docs).post(create_doc),
        )
        .route("/api/projects/{slug}/docs/{doc_id}/content", get(get_doc_content))
        .route(
            "/api/projects/{slug}/docs/{doc_id}",
            axum::routing::put(save_doc)
                .patch(move_doc)
                .delete(delete_doc),
        )
        .route("/api/projects/{slug}/docs/{doc_id}/history", get(doc_history))
        .route("/api/projects/{slug}/docs/{doc_id}/diff", get(doc_diff))
        .route("/api/projects/{slug}/docs/{doc_id}/revert", axum::routing::post(revert_doc))
        .route("/api/projects/{slug}/docs/{doc_id}/publish", axum::routing::patch(set_published))
        .route("/api/projects/{slug}/search", get(search_docs))
        .route("/api/projects/{slug}/links", get(get_links))
}

#[derive(Deserialize)]
pub struct BranchQuery {
    pub branch: Option<String>,
}

#[derive(Serialize)]
struct DocListItem {
    id: Uuid,
    path: String,
    title: String,
    published: bool,
    version_id: Option<Uuid>,
    updated_at: DateTime<Utc>,
    size_bytes: i64,
}

#[derive(Serialize)]
struct BranchInfo {
    id: Uuid,
    name: String,
    is_default: bool,
}

#[derive(Serialize)]
struct DocsDto {
    branch: BranchInfo,
    docs: Vec<DocListItem>,
}

async fn list_docs(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
    Query(q): Query<BranchQuery>,
) -> AppResult<Json<DocsDto>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Public).await?;
    let br = vcs::resolve_branch(&state.db, &proj, q.branch.as_deref()).await?;

    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let states = vcs::branch_doc_states(&state.db, &graph, br.head_commit_id).await?;
    let alive: Vec<(Uuid, Option<Uuid>)> = states
        .values()
        .filter(|s| s.change_type != "deleted")
        .map(|s| (s.document_id, s.version_id))
        .collect();

    if alive.is_empty() {
        return Ok(Json(DocsDto {
            branch: BranchInfo {
                id: br.id,
                name: br.name,
                is_default: br.is_default,
            },
            docs: vec![],
        }));
    }

    let doc_ids: Vec<Uuid> = alive.iter().map(|(id, _)| *id).collect();
    let docs = document::Entity::find()
        .filter(document::Column::ProjectId.eq(proj.id))
        .filter(document::Column::Id.is_in(doc_ids))
        .all(&state.db)
        .await?;
    let by_id: std::collections::HashMap<Uuid, document::Model> =
        docs.into_iter().map(|d| (d.id, d)).collect();

    let ver_ids: Vec<Uuid> = alive.iter().filter_map(|(_, v)| *v).collect();
    let sizes: std::collections::HashMap<Uuid, i32> = if ver_ids.is_empty() {
        Default::default()
    } else {
        doc_version::Entity::find()
            .filter(doc_version::Column::Id.is_in(ver_ids))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|v| (v.id, v.size_bytes))
            .collect()
    };

    let mut items: Vec<DocListItem> = Vec::with_capacity(alive.len());
    for (doc_id, version_id) in alive {
        let Some(d) = by_id.get(&doc_id) else { continue };
        items.push(DocListItem {
            id: d.id,
            path: d.path.clone(),
            title: d.title.clone(),
            published: d.published,
            version_id,
            updated_at: states.get(&doc_id).map(|s| s.updated_at).unwrap_or(d.updated_at),
            size_bytes: version_id
                .and_then(|v| sizes.get(&v).copied())
                .unwrap_or(0) as i64,
        });
    }
    items.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Json(DocsDto {
        branch: BranchInfo {
            id: br.id,
            name: br.name,
            is_default: br.is_default,
        },
        docs: items,
    }))
}

#[derive(Deserialize)]
struct CreateDocReq {
    path: String,
    content: String,
    branch: Option<String>,
    message: Option<String>,
}

async fn create_doc(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
    Json(req): Json<CreateDocReq>,
) -> AppResult<Json<serde_json::Value>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Editor).await?;
    let path = req.path.trim().trim_start_matches("./").to_string();
    vcs::validate_doc_path(&path)?;
    let br = vcs::resolve_branch(&state.db, &proj, req.branch.as_deref()).await?;

    // path must not be taken by a live doc on this branch
    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let states = vcs::branch_doc_states(&state.db, &graph, br.head_commit_id).await?;
    check_path_free(&state.db, &proj, &states, &path, None).await?;

    let now = Utc::now();
    let title = vcs::extract_title(&req.content, &path);
    let txn = state.db.begin().await?;

    // reuse the documents row if the path existed before (deleted doc)
    let doc = match document::Entity::find()
        .filter(document::Column::ProjectId.eq(proj.id))
        .filter(document::Column::Path.eq(&path))
        .one(&txn)
        .await?
    {
        Some(existing) => {
            let mut am = existing.into_active_model();
            am.title = Set(title.clone());
            am.updated_at = Set(now);
            am.update(&txn).await?
        }
        None => document::ActiveModel {
            id: Set(Uuid::now_v7()),
            project_id: Set(proj.id),
            path: Set(path.clone()),
            title: Set(title.clone()),
            published: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&txn)
        .await?,
    };

    let ver = vcs::create_version(&txn, doc.id, &req.content, cur.id).await?;
    let commit = vcs::create_commit(
        &txn,
        proj.id,
        br.id,
        br.head_commit_id,
        cur.id,
        &req
            .message
            .clone()
            .unwrap_or_else(|| format!("create {path}")),
        "edit",
        vec![vcs::Change {
            document_id: doc.id,
            change_type: "added",
            version_id: Some(ver.id),
            old_path: None,
        }],
    )
    .await?;
    links::after_commit(
        &txn,
        proj.id,
        br.id,
        Some(commit.id),
        &[LinkUpdate {
            doc_id: doc.id,
            path: &path,
            content: Some(&req.content),
        }],
    )
    .await?;
    txn.commit().await?;

    Ok(Json(serde_json::json!({
        "id": doc.id,
        "path": path,
        "title": title,
        "version_id": ver.id,
        "commit_id": commit.id,
    })))
}

/// Reject a path that collides with a live document: either the very same path,
/// or the same file name under another directory (which would put a second copy
/// of an existing document in the project). `exclude` skips the document that is
/// being renamed.
async fn check_path_free(
    db: &sea_orm::DatabaseConnection,
    proj: &crate::entities::project::Model,
    states: &std::collections::HashMap<Uuid, vcs::DocState>,
    path: &str,
    exclude: Option<Uuid>,
) -> AppResult<()> {
    let alive: Vec<Uuid> = states
        .values()
        .filter(|s| s.change_type != "deleted")
        .map(|s| s.document_id)
        .filter(|id| Some(*id) != exclude)
        .collect();
    if alive.is_empty() {
        return Ok(());
    }
    let docs = document::Entity::find()
        .filter(document::Column::ProjectId.eq(proj.id))
        .filter(document::Column::Id.is_in(alive))
        .all(db)
        .await?;
    if docs.iter().any(|d| d.path == path) {
        return Err(AppError::Conflict(format!(
            "a document already exists at this path: {path}"
        )));
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    if let Some(same) = docs
        .iter()
        .find(|d| d.path.rsplit('/').next().unwrap_or(d.path.as_str()) == name)
    {
        return Err(AppError::Conflict(format!(
            "another document with the same file name exists in this project: {}",
            same.path
        )));
    }
    Ok(())
}

async fn load_doc(
    db: &sea_orm::DatabaseConnection,
    proj: &crate::entities::project::Model,
    doc_id: Uuid,
) -> AppResult<document::Model> {
    document::Entity::find_by_id(doc_id)
        .one(db)
        .await?
        .filter(|d| d.project_id == proj.id)
        .ok_or_else(|| AppError::NotFound("document not found".into()))
}

#[derive(Serialize)]
struct DocContentDto {
    id: Uuid,
    path: String,
    title: String,
    content: String,
    version_id: Uuid,
    commit_id: Uuid,
    size_bytes: i32,
    updated_at: DateTime<Utc>,
}

async fn get_doc_content(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path((slug, doc_id)): Path<(String, Uuid)>,
    Query(q): Query<BranchQuery>,
) -> AppResult<Json<DocContentDto>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Public).await?;
    let br = vcs::resolve_branch(&state.db, &proj, q.branch.as_deref()).await?;
    let doc = load_doc(&state.db, &proj, doc_id).await?;

    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let st = vcs::doc_state(&state.db, &graph, br.head_commit_id, doc_id)
        .await?
        .filter(|s| s.change_type != "deleted")
        .ok_or_else(|| AppError::NotFound("document not on this branch".into()))?;
    let Some(version_id) = st.version_id else {
        return Err(AppError::NotFound("document has no content".into()));
    };
    let ver = doc_version::Entity::find_by_id(version_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::Internal("version missing".into()))?;

    Ok(Json(DocContentDto {
        id: doc.id,
        path: doc.path,
        title: doc.title,
        content: ver.content,
        version_id: ver.id,
        commit_id: st.commit_id,
        size_bytes: ver.size_bytes,
        updated_at: st.updated_at,
    }))
}

#[derive(Deserialize)]
struct SaveDocReq {
    content: String,
    message: Option<String>,
    branch: Option<String>,
    base_version_id: Option<Uuid>,
}

async fn save_doc(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path((slug, doc_id)): Path<(String, Uuid)>,
    Json(req): Json<SaveDocReq>,
) -> AppResult<Json<serde_json::Value>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Editor).await?;
    let br = vcs::resolve_branch(&state.db, &proj, req.branch.as_deref()).await?;
    let doc = load_doc(&state.db, &proj, doc_id).await?;

    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let st = vcs::doc_state(&state.db, &graph, br.head_commit_id, doc_id)
        .await?
        .filter(|s| s.change_type != "deleted")
        .ok_or_else(|| AppError::NotFound("document not on this branch".into()))?;

    // optimistic lock: client must have based its edit on the current version
    if let Some(base) = req.base_version_id {
        if st.version_id != Some(base) {
            return Err(AppError::Conflict(
                "document changed by someone else, reload and retry".into(),
            ));
        }
    }

    // no-op when content is identical
    let Some(cur_vid) = st.version_id else {
        return Err(AppError::NotFound("document has no content".into()));
    };
    let cur_ver = doc_version::Entity::find_by_id(cur_vid)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::Internal("version missing".into()))?;
    if cur_ver.content_hash == vcs::sha256_hex(&req.content) {
        return Ok(Json(serde_json::json!({
            "id": doc.id, "version_id": st.version_id, "commit_id": st.commit_id,
            "unchanged": true,
        })));
    }

    let now = Utc::now();
    let doc_path = doc.path.clone();
    let title = vcs::extract_title(&req.content, &doc.path);
    let txn = state.db.begin().await?;
    let ver = vcs::create_version(&txn, doc.id, &req.content, cur.id).await?;
    let commit = vcs::create_commit(
        &txn,
        proj.id,
        br.id,
        br.head_commit_id,
        cur.id,
        &req.message.clone().unwrap_or_else(|| format!("edit {}", doc.path)),
        "edit",
        vec![vcs::Change {
            document_id: doc.id,
            change_type: "modified",
            version_id: Some(ver.id),
            old_path: None,
        }],
    )
    .await?;

    let mut am: document::ActiveModel = doc.into_active_model();
    am.title = Set(title);
    am.updated_at = Set(now);
    am.update(&txn).await?;

    links::after_commit(
        &txn,
        proj.id,
        br.id,
        Some(commit.id),
        &[LinkUpdate {
            doc_id,
            path: &doc_path,
            content: Some(&req.content),
        }],
    )
    .await?;

    txn.commit().await?;
    Ok(Json(serde_json::json!({
        "id": doc_id,
        "version_id": ver.id,
        "commit_id": commit.id,
        "updated_at": now,
    })))
}

#[derive(Deserialize)]
struct MoveDocReq {
    new_path: String,
    branch: Option<String>,
    message: Option<String>,
}

async fn move_doc(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path((slug, doc_id)): Path<(String, Uuid)>,
    Json(req): Json<MoveDocReq>,
) -> AppResult<Json<serde_json::Value>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Editor).await?;
    let new_path = req.new_path.trim().trim_start_matches("./").to_string();
    vcs::validate_doc_path(&new_path)?;
    let br = vcs::resolve_branch(&state.db, &proj, req.branch.as_deref()).await?;
    let doc = load_doc(&state.db, &proj, doc_id).await?;

    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let states = vcs::branch_doc_states(&state.db, &graph, br.head_commit_id).await?;
    let st = states
        .get(&doc_id)
        .filter(|s| s.change_type != "deleted")
        .ok_or_else(|| AppError::NotFound("document not on this branch".into()))?;
    if doc.path == new_path {
        return Ok(Json(serde_json::json!({ "id": doc_id, "path": new_path, "unchanged": true })));
    }
    check_path_free(&state.db, &proj, &states, &new_path, Some(doc_id)).await?;

    let old_path = doc.path.clone();
    let now = Utc::now();
    let txn = state.db.begin().await?;
    let commit = vcs::create_commit(
        &txn,
        proj.id,
        br.id,
        br.head_commit_id,
        cur.id,
        &req
            .message
            .clone()
            .unwrap_or_else(|| format!("move {old_path} -> {new_path}")),
        "edit",
        vec![vcs::Change {
            document_id: doc.id,
            change_type: "renamed",
            version_id: st.version_id,
            old_path: Some(old_path.clone()),
        }],
    )
    .await?;

    let mut am: document::ActiveModel = doc.into_active_model();
    am.path = Set(new_path.clone());
    am.updated_at = Set(now);
    am.update(&txn).await?;

    // relative links resolve against the new location: re-extract this doc's links
    let content = match st.version_id {
        Some(vid) => doc_version::Entity::find_by_id(vid)
            .one(&txn)
            .await?
            .map(|v| v.content),
        None => None,
    };
    links::after_commit(
        &txn,
        proj.id,
        br.id,
        Some(commit.id),
        &[LinkUpdate {
            doc_id,
            path: &new_path,
            content: content.as_deref(),
        }],
    )
    .await?;

    txn.commit().await?;
    Ok(Json(serde_json::json!({
        "id": doc_id, "path": new_path, "commit_id": commit.id,
    })))
}

async fn delete_doc(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path((slug, doc_id)): Path<(String, Uuid)>,
    Query(q): Query<BranchQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Editor).await?;
    let br = vcs::resolve_branch(&state.db, &proj, q.branch.as_deref()).await?;
    let doc = load_doc(&state.db, &proj, doc_id).await?;

    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let alive = vcs::doc_state(&state.db, &graph, br.head_commit_id, doc_id)
        .await?
        .is_some_and(|s| s.change_type != "deleted");
    if !alive {
        return Err(AppError::NotFound("document not on this branch".into()));
    }
    let path = doc.path.clone();
    let txn = state.db.begin().await?;
    let commit = vcs::create_commit(
        &txn,
        proj.id,
        br.id,
        br.head_commit_id,
        cur.id,
        &format!("delete {path}"),
        "edit",
        vec![vcs::Change {
            document_id: doc.id,
            change_type: "deleted",
            version_id: None,
            old_path: Some(path.clone()),
        }],
    )
    .await?;
    links::after_commit(
        &txn,
        proj.id,
        br.id,
        Some(commit.id),
        &[LinkUpdate {
            doc_id,
            path: &path,
            content: None,
        }],
    )
    .await?;
    txn.commit().await?;
    Ok(Json(serde_json::json!({ "id": doc_id, "commit_id": commit.id })))
}

// ---------- version history / diff / revert ----------

#[derive(Serialize)]
struct RevisionDto {
    commit_id: Uuid,
    message: String,
    commit_type: String,
    author: String,
    created_at: DateTime<Utc>,
    change_type: String,
    version_id: Option<Uuid>,
    size_bytes: i64,
    /// true when this revision is the live one on the requested branch
    current: bool,
}

async fn doc_history(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path((slug, doc_id)): Path<(String, Uuid)>,
    Query(q): Query<BranchQuery>,
) -> AppResult<Json<Vec<RevisionDto>>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Public).await?;
    let br = vcs::resolve_branch(&state.db, &proj, q.branch.as_deref()).await?;
    load_doc(&state.db, &proj, doc_id).await?;

    let Some(head) = br.head_commit_id else {
        return Ok(Json(vec![]));
    };
    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let chain = graph.chain(head);
    let pos: std::collections::HashMap<Uuid, usize> =
        chain.iter().enumerate().map(|(i, id)| (*id, i)).collect();

    let cds = commit_doc::Entity::find()
        .filter(commit_doc::Column::DocumentId.eq(doc_id))
        .filter(commit_doc::Column::CommitId.is_in(chain))
        .all(&state.db)
        .await?;
    if cds.is_empty() {
        return Ok(Json(vec![]));
    }

    let commit_ids: Vec<Uuid> = cds.iter().map(|c| c.commit_id).collect();
    let commits = commit::Entity::find()
        .filter(commit::Column::Id.is_in(commit_ids))
        .all(&state.db)
        .await?;
    let by_id: std::collections::HashMap<Uuid, commit::Model> =
        commits.into_iter().map(|c| (c.id, c)).collect();

    let author_ids: Vec<Uuid> = by_id.values().map(|c| c.author_id).collect();
    let users = user::Entity::find()
        .filter(user::Column::Id.is_in(author_ids))
        .all(&state.db)
        .await?;
    let by_user: std::collections::HashMap<Uuid, user::Model> =
        users.into_iter().map(|u| (u.id, u)).collect();

    let ver_ids: Vec<Uuid> = cds.iter().filter_map(|c| c.version_id).collect();
    let sizes: std::collections::HashMap<Uuid, i32> = if ver_ids.is_empty() {
        Default::default()
    } else {
        doc_version::Entity::find()
            .filter(doc_version::Column::Id.is_in(ver_ids))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|v| (v.id, v.size_bytes))
            .collect()
    };

    let current_commit = vcs::doc_state(&state.db, &graph, Some(head), doc_id)
        .await?
        .map(|s| s.commit_id);

    let mut out: Vec<RevisionDto> = cds
        .into_iter()
        .map(|cd| {
            let c = by_id.get(&cd.commit_id);
            let author = c
                .and_then(|c| by_user.get(&c.author_id))
                .map(|u| {
                    if u.display_name.is_empty() {
                        u.username.clone()
                    } else {
                        u.display_name.clone()
                    }
                })
                .unwrap_or_default();
            RevisionDto {
                commit_id: cd.commit_id,
                message: c.map(|c| c.message.clone()).unwrap_or_default(),
                commit_type: c.map(|c| c.commit_type.clone()).unwrap_or_default(),
                author,
                created_at: c.map(|c| c.created_at).unwrap_or_default(),
                change_type: cd.change_type.clone(),
                version_id: cd.version_id,
                size_bytes: cd.version_id.and_then(|v| sizes.get(&v).copied()).unwrap_or(0) as i64,
                current: Some(cd.commit_id) == current_commit,
            }
        })
        .collect();
    // chain index 0 is the head => ascending order yields newest first
    out.sort_by_key(|r| pos.get(&r.commit_id).copied().unwrap_or(usize::MAX));
    Ok(Json(out))
}

/// Content of a document as recorded by a specific commit (None => absent/deleted).
async fn content_at_commit(
    db: &sea_orm::DatabaseConnection,
    doc_id: Uuid,
    commit_id: Uuid,
) -> AppResult<Option<String>> {
    let cd = commit_doc::Entity::find()
        .filter(commit_doc::Column::DocumentId.eq(doc_id))
        .filter(commit_doc::Column::CommitId.eq(commit_id))
        .one(db)
        .await?;
    let Some(cd) = cd else { return Ok(None) };
    let Some(vid) = cd.version_id else { return Ok(None) };
    Ok(doc_version::Entity::find_by_id(vid)
        .one(db)
        .await?
        .map(|v| v.content))
}

#[derive(Deserialize)]
struct DiffQuery {
    branch: Option<String>,
    from: Option<Uuid>,
    to: Option<Uuid>,
    context: Option<usize>,
}

#[derive(Serialize)]
struct DiffDto {
    branch: String,
    from_commit: Option<Uuid>,
    to_commit: Option<Uuid>,
    from_label: String,
    to_label: String,
    has_changes: bool,
    diff: String,
    old_content: String,
    new_content: String,
}

fn short_id(id: Option<Uuid>) -> String {
    match id {
        Some(u) => u.to_string().chars().take(8).collect(),
        None => "current".to_string(),
    }
}

async fn doc_diff(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path((slug, doc_id)): Path<(String, Uuid)>,
    Query(q): Query<DiffQuery>,
) -> AppResult<Json<DiffDto>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Public).await?;
    let br = vcs::resolve_branch(&state.db, &proj, q.branch.as_deref()).await?;
    load_doc(&state.db, &proj, doc_id).await?;

    let old_content = match q.from {
        Some(c) => content_at_commit(&state.db, doc_id, c).await?.unwrap_or_default(),
        None => String::new(),
    };
    let new_content = match q.to {
        Some(c) => content_at_commit(&state.db, doc_id, c).await?.unwrap_or_default(),
        None => {
            let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
            let st = vcs::doc_state(&state.db, &graph, br.head_commit_id, doc_id).await?;
            match st.and_then(|s| s.version_id) {
                Some(vid) => doc_version::Entity::find_by_id(vid)
                    .one(&state.db)
                    .await?
                    .map(|v| v.content)
                    .unwrap_or_default(),
                None => String::new(),
            }
        }
    };

    let ctx = q.context.unwrap_or(3).min(20);
    let text = diff::unified(&old_content, &new_content, ctx);
    Ok(Json(DiffDto {
        branch: br.name,
        from_commit: q.from,
        to_commit: q.to,
        from_label: short_id(q.from),
        to_label: short_id(q.to),
        has_changes: !text.is_empty(),
        diff: text,
        old_content,
        new_content,
    }))
}

// ---------- 全文检索 ----------

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    branch: Option<String>,
    /// 最多返回多少条命中（默认 50，上限 200）
    limit: Option<usize>,
}

#[derive(Serialize)]
struct SearchDto {
    branch: BranchInfo,
    query: String,
    /// 全部命中数，可能大于返回的条数
    total: usize,
    truncated: bool,
    hits: Vec<search::Hit>,
}

/// 在当前分支的所有文档正文里做大小写不敏感的字面量检索。
async fn search_docs(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<SearchDto>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Public).await?;
    let br = vcs::resolve_branch(&state.db, &proj, q.branch.as_deref()).await?;
    let branch = BranchInfo {
        id: br.id,
        name: br.name.clone(),
        is_default: br.is_default,
    };

    let term = q.q.trim();
    if term.is_empty() {
        return Err(AppError::BadRequest("检索词不能为空".into()));
    }
    let limit = q.limit.unwrap_or(50).clamp(1, 200);

    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let states = vcs::branch_doc_states(&state.db, &graph, br.head_commit_id).await?;
    let alive: Vec<(Uuid, Uuid)> = states
        .values()
        .filter(|s| s.change_type != "deleted")
        .filter_map(|s| s.version_id.map(|v| (s.document_id, v)))
        .collect();

    let (total, hits, truncated) = if alive.is_empty() {
        (0, vec![], false)
    } else {
        let doc_ids: Vec<Uuid> = alive.iter().map(|(id, _)| *id).collect();
        let docs = document::Entity::find()
            .filter(document::Column::ProjectId.eq(proj.id))
            .filter(document::Column::Id.is_in(doc_ids))
            .all(&state.db)
            .await?;
        let by_id: std::collections::HashMap<Uuid, document::Model> =
            docs.into_iter().map(|d| (d.id, d)).collect();

        let ver_ids: Vec<Uuid> = alive.iter().map(|(_, v)| *v).collect();
        let contents: std::collections::HashMap<Uuid, String> = doc_version::Entity::find()
            .filter(doc_version::Column::Id.is_in(ver_ids))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|v| (v.id, v.content))
            .collect();

        // 按路径排序，结果顺序稳定
        let mut ordered = alive;
        ordered.sort_by_key(|(id, _)| {
            by_id.get(id).map(|d| d.path.clone()).unwrap_or_default()
        });

        let mut texts: Vec<search::DocText> = Vec::new();
        for (doc_id, version_id) in ordered {
            let Some(d) = by_id.get(&doc_id) else { continue };
            let Some(content) = contents.get(&version_id) else {
                continue;
            };
            texts.push(search::DocText {
                doc_id: d.id,
                path: &d.path,
                title: &d.title,
                content,
            });
        }
        search::run(texts, term, limit)
    };

    Ok(Json(SearchDto {
        branch,
        query: term.to_string(),
        total,
        truncated,
        hits,
    }))
}

#[derive(Deserialize)]
struct RevertReq {
    /// the commit whose document state should be restored
    commit_id: Uuid,
    branch: Option<String>,
    message: Option<String>,
}

async fn revert_doc(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path((slug, doc_id)): Path<(String, Uuid)>,
    Json(req): Json<RevertReq>,
) -> AppResult<Json<serde_json::Value>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Editor).await?;
    let br = vcs::resolve_branch(&state.db, &proj, req.branch.as_deref()).await?;
    let doc = load_doc(&state.db, &proj, doc_id).await?;

    let restored = content_at_commit(&state.db, doc_id, req.commit_id)
        .await?
        .ok_or_else(|| {
            AppError::BadRequest(
                "revision not found for this document, or it was a deletion".into(),
            )
        })?;

    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let st = vcs::doc_state(&state.db, &graph, br.head_commit_id, doc_id).await?;
    let alive = st.as_ref().is_some_and(|s| s.change_type != "deleted");

    // no-op if the target revision already matches the branch state
    if let Some(cur_vid) = st.as_ref().and_then(|s| s.version_id) {
        if let Some(cur_ver) = doc_version::Entity::find_by_id(cur_vid).one(&state.db).await? {
            if cur_ver.content_hash == vcs::sha256_hex(&restored) {
                return Ok(Json(serde_json::json!({
                    "id": doc_id, "unchanged": true, "version_id": cur_vid,
                })));
            }
        }
    }

    let path = doc.path.clone();
    let title = vcs::extract_title(&restored, &path);
    let now = Utc::now();
    let txn = state.db.begin().await?;
    let ver = vcs::create_version(&txn, doc_id, &restored, cur.id).await?;
    let message = req
        .message
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "revert {path} to {}",
                short_id(Some(req.commit_id))
            )
        });
    let commit = vcs::create_commit(
        &txn,
        proj.id,
        br.id,
        br.head_commit_id,
        cur.id,
        &message,
        "revert",
        vec![vcs::Change {
            document_id: doc_id,
            change_type: if alive { "modified" } else { "added" },
            version_id: Some(ver.id),
            old_path: None,
        }],
    )
    .await?;

    let mut am: document::ActiveModel = doc.into_active_model();
    am.title = Set(title);
    am.updated_at = Set(now);
    am.update(&txn).await?;

    links::after_commit(
        &txn,
        proj.id,
        br.id,
        Some(commit.id),
        &[LinkUpdate {
            doc_id,
            path: &path,
            content: Some(&restored),
        }],
    )
    .await?;
    txn.commit().await?;

    Ok(Json(serde_json::json!({
        "id": doc_id,
        "version_id": ver.id,
        "commit_id": commit.id,
        "restored_from": req.commit_id,
        "branch": br.name,
    })))
}

// ---------- public visibility ----------

#[derive(Deserialize)]
struct PublishReq {
    published: bool,
}

/// Toggle whether a document is listed on the public site. Publishing is
/// metadata only (no commit) — content history stays untouched.
async fn set_published(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path((slug, doc_id)): Path<(String, Uuid)>,
    Json(req): Json<PublishReq>,
) -> AppResult<Json<serde_json::Value>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Editor).await?;
    let doc = load_doc(&state.db, &proj, doc_id).await?;

    let mut am: document::ActiveModel = doc.into_active_model();
    am.published = Set(req.published);
    am.updated_at = Set(Utc::now());
    let updated = am.update(&state.db).await?;

    Ok(Json(serde_json::json!({
        "id": updated.id,
        "published": updated.published,
        "project_public": proj.visibility == "public",
    })))
}

// ---------- link graph ----------
#[derive(Serialize)]
struct LinkNodeDto {
    id: Uuid,
    path: String,
    title: String,
    out: usize,
    inn: usize,
    dangling: usize,
}

#[derive(Serialize)]
struct LinkEdgeDto {
    source: Uuid,
    target: Option<Uuid>,
    raw_target: String,
    #[serde(rename = "type")]
    link_type: String,
    anchor: Option<String>,
}

#[derive(Serialize)]
struct LinksDto {
    branch: BranchInfo,
    nodes: Vec<LinkNodeDto>,
    edges: Vec<LinkEdgeDto>,
}

async fn get_links(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
    Query(q): Query<BranchQuery>,
) -> AppResult<Json<LinksDto>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Public).await?;
    let br = vcs::resolve_branch(&state.db, &proj, q.branch.as_deref()).await?;

    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let states = vcs::branch_doc_states(&state.db, &graph, br.head_commit_id).await?;
    let alive: Vec<Uuid> = states
        .values()
        .filter(|s| s.change_type != "deleted")
        .map(|s| s.document_id)
        .collect();

    let edges = link_edge::Entity::find()
        .filter(link_edge::Column::BranchId.eq(br.id))
        .all(&state.db)
        .await?;

    let docs = if alive.is_empty() {
        vec![]
    } else {
        document::Entity::find()
            .filter(document::Column::ProjectId.eq(proj.id))
            .filter(document::Column::Id.is_in(alive))
            .all(&state.db)
            .await?
    };

    let mut out = vec![0usize; docs.len()];
    let mut inn = vec![0usize; docs.len()];
    let mut dangling = vec![0usize; docs.len()];
    let idx: std::collections::HashMap<Uuid, usize> =
        docs.iter().enumerate().map(|(i, d)| (d.id, i)).collect();

    let edge_dtos: Vec<LinkEdgeDto> = edges
        .iter()
        .map(|e| {
            if let Some(si) = idx.get(&e.source_doc_id) {
                out[*si] += 1;
                if e.target_doc_id.is_none() {
                    dangling[*si] += 1;
                }
            }
            if let Some(&ti) = e.target_doc_id.as_ref().and_then(|t| idx.get(t)) {
                inn[ti] += 1;
            }
            LinkEdgeDto {
                source: e.source_doc_id,
                target: e.target_doc_id,
                raw_target: e.raw_target.clone(),
                link_type: e.link_type.clone(),
                anchor: e.anchor.clone(),
            }
        })
        .collect();

    let nodes: Vec<LinkNodeDto> = docs
        .iter()
        .enumerate()
        .map(|(i, d)| LinkNodeDto {
            id: d.id,
            path: d.path.clone(),
            title: d.title.clone(),
            out: out[i],
            inn: inn[i],
            dangling: dangling[i],
        })
        .collect();

    Ok(Json(LinksDto {
        branch: BranchInfo {
            id: br.id,
            name: br.name,
            is_default: br.is_default,
        },
        nodes,
        edges: edge_dtos,
    }))
}
