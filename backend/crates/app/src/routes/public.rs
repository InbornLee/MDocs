//! Anonymous, read-only endpoints powering the public homepage.
//!
//! Everything here is mounted on the *public* router (no auth middleware).
//! Only projects with `visibility = "public"` are reachable, and only their
//! published documents are exposed.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use sea_orm::{
    ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{branch, commit, commit_doc, document, doc_version, link_edge, project, user};
use crate::error::{AppError, AppResult};
use crate::services::{diff, search, site, vcs};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/public/site", get(get_site))
        .route("/api/public/projects", get(list_projects))
        .route("/api/public/projects/{slug}", get(get_project))
        .route(
            "/api/public/projects/{slug}/docs/{doc_id}/content",
            get(get_doc_content),
        )
        .route("/api/public/projects/{slug}/links", get(get_links))
        .route(
            "/api/public/projects/{slug}/docs/{doc_id}/history",
            get(doc_history),
        )
        .route(
            "/api/public/projects/{slug}/docs/{doc_id}/diff",
            get(doc_diff),
        )
        .route("/api/public/projects/{slug}/search", get(search_docs))
}

/// Fetch a project that is publicly browsable, else 404.
pub(crate) async fn public_project(state: &AppState, slug: &str) -> AppResult<project::Model> {
    project::Entity::find()
        .filter(project::Column::Slug.eq(slug))
        .one(&state.db)
        .await?
        .filter(|p| p.visibility == "public")
        .ok_or_else(|| AppError::NotFound("public project not found".into()))
}

pub(crate) async fn default_branch(
    state: &AppState,
    proj: &project::Model,
    name: Option<&str>,
) -> AppResult<branch::Model> {
    match name {
        Some(n) => vcs::resolve_branch(&state.db, proj, Some(n)).await,
        None => vcs::resolve_branch(&state.db, proj, None).await,
    }
}

fn owner_name(u: &user::Model) -> String {
    if u.display_name.is_empty() {
        u.username.clone()
    } else {
        u.display_name.clone()
    }
}

/// Human-readable name for a user id.
pub(crate) async fn display_name(state: &AppState, id: Uuid) -> AppResult<String> {
    Ok(user::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .map(|u| owner_name(&u))
        .unwrap_or_default())
}

// ---------- listing ----------

/// 站点级公开配置（首页标题、仓库与反馈入口）。管理员在管理界面里改，这里只读。
async fn get_site(State(state): State<AppState>) -> AppResult<Json<site::SiteInfo>> {
    Ok(Json(site::site_info(&state.db).await?))
}

#[derive(Deserialize)]
struct ListQuery {
    query: Option<String>,
    page: Option<u64>,
    page_size: Option<u64>,
}

#[derive(Serialize)]
struct PublicProjectItem {
    id: Uuid,
    slug: String,
    name: String,
    description: Option<String>,
    owner_name: String,
    doc_count: usize,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct PublicProjectList {
    items: Vec<PublicProjectItem>,
    total: usize,
    page: u64,
    page_size: u64,
}

/// Published + alive document ids of a project's default branch.
pub(crate) async fn published_docs(
    state: &AppState,
    proj: &project::Model,
    head: Option<Uuid>,
) -> AppResult<Vec<document::Model>> {
    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let states = vcs::branch_doc_states(&state.db, &graph, head).await?;
    let alive: Vec<Uuid> = states
        .values()
        .filter(|s| s.change_type != "deleted")
        .map(|s| s.document_id)
        .collect();
    if alive.is_empty() {
        return Ok(vec![]);
    }
    Ok(document::Entity::find()
        .filter(document::Column::ProjectId.eq(proj.id))
        .filter(document::Column::Id.is_in(alive))
        .filter(document::Column::Published.eq(true))
        .all(&state.db)
        .await?)
}

async fn list_projects(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<PublicProjectList>> {
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(12).clamp(1, 50);

    let mut cond = Condition::all().add(project::Column::Visibility.eq("public"));
    if let Some(kw) = q.query.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let like = format!("%{kw}%");
        cond = cond.add(
            Condition::any()
                .add(project::Column::Name.like(&like))
                .add(project::Column::Slug.like(&like))
                .add(project::Column::Description.like(&like)),
        );
    }

    let total = project::Entity::find()
        .filter(cond.clone())
        .count(&state.db)
        .await? as usize;

    let rows = project::Entity::find()
        .filter(cond)
        .order_by_desc(project::Column::UpdatedAt)
        .paginate(&state.db, page_size)
        .fetch_page(page - 1)
        .await?;

    let owner_ids: Vec<Uuid> = rows.iter().map(|p| p.owner_id).collect();
    let owners = if owner_ids.is_empty() {
        vec![]
    } else {
        user::Entity::find()
            .filter(user::Column::Id.is_in(owner_ids))
            .all(&state.db)
            .await?
    };
    let by_owner: HashMap<Uuid, user::Model> = owners.into_iter().map(|u| (u.id, u)).collect();

    let mut items = Vec::with_capacity(rows.len());
    for p in rows {
        let br = match p.default_branch_id {
            Some(bid) => branch::Entity::find_by_id(bid).one(&state.db).await?,
            None => None,
        };
        let docs = published_docs(&state, &p, br.and_then(|b| b.head_commit_id)).await?;
        items.push(PublicProjectItem {
            id: p.id,
            slug: p.slug.clone(),
            name: p.name.clone(),
            description: p.description.clone(),
            owner_name: by_owner.get(&p.owner_id).map(owner_name).unwrap_or_default(),
            doc_count: docs.len(),
            updated_at: p.updated_at,
        });
    }

    Ok(Json(PublicProjectList {
        items,
        total,
        page,
        page_size,
    }))
}

// ---------- project detail ----------

#[derive(Serialize)]
struct PublicDocItem {
    id: Uuid,
    path: String,
    title: String,
    size_bytes: i64,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct PublicBranchItem {
    id: Uuid,
    name: String,
    is_default: bool,
}

#[derive(Serialize)]
struct PublicProjectDetail {
    id: Uuid,
    slug: String,
    name: String,
    description: Option<String>,
    owner_name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    branch: PublicBranchItem,
    branches: Vec<PublicBranchItem>,
    docs: Vec<PublicDocItem>,
    link_summary: LinkSummary,
}

#[derive(Serialize, Default)]
struct LinkSummary {
    nodes: usize,
    edges: usize,
    dangling: usize,
}

async fn get_project(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(q): Query<BranchQueryArg>,
) -> AppResult<Json<PublicProjectDetail>> {
    let proj = public_project(&state, &slug).await?;
    let br = default_branch(&state, &proj, q.branch.as_deref()).await?;

    let branches = branch::Entity::find()
        .filter(branch::Column::ProjectId.eq(proj.id))
        .all(&state.db)
        .await?;

    let docs = published_docs(&state, &proj, br.head_commit_id).await?;
    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let states = vcs::branch_doc_states(&state.db, &graph, br.head_commit_id).await?;

    let ver_ids: Vec<Uuid> = states
        .values()
        .filter(|s| s.change_type != "deleted")
        .filter_map(|s| s.version_id)
        .collect();
    let sizes: HashMap<Uuid, i32> = if ver_ids.is_empty() {
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

    let mut items: Vec<PublicDocItem> = docs
        .iter()
        .map(|d| PublicDocItem {
            id: d.id,
            path: d.path.clone(),
            title: d.title.clone(),
            size_bytes: states
                .get(&d.id)
                .and_then(|s| s.version_id)
                .and_then(|v| sizes.get(&v).copied())
                .unwrap_or(0) as i64,
            updated_at: states.get(&d.id).map(|s| s.updated_at).unwrap_or(d.updated_at),
        })
        .collect();
    items.sort_by(|a, b| a.path.cmp(&b.path));

    // link summary over published docs only
    let edges = link_edge::Entity::find()
        .filter(link_edge::Column::BranchId.eq(br.id))
        .all(&state.db)
        .await?;
    let doc_ids: Vec<Uuid> = items.iter().map(|d| d.id).collect();
    let visible_edges: Vec<_> = edges
        .iter()
        .filter(|e| doc_ids.contains(&e.source_doc_id))
        .collect();
    let link_summary = LinkSummary {
        nodes: items.len(),
        edges: visible_edges.len(),
        dangling: visible_edges.iter().filter(|e| e.target_doc_id.is_none()).count(),
    };

    let owner = user::Entity::find_by_id(proj.owner_id).one(&state.db).await?;

    Ok(Json(PublicProjectDetail {
        id: proj.id,
        slug: proj.slug.clone(),
        name: proj.name.clone(),
        description: proj.description.clone(),
        owner_name: owner.as_ref().map(owner_name).unwrap_or_default(),
        created_at: proj.created_at,
        updated_at: proj.updated_at,
        branch: PublicBranchItem {
            id: br.id,
            name: br.name.clone(),
            is_default: br.is_default,
        },
        branches: branches
            .into_iter()
            .map(|b| PublicBranchItem {
                id: b.id,
                name: b.name,
                is_default: b.is_default,
            })
            .collect(),
        docs: items,
        link_summary,
    }))
}

#[derive(Deserialize)]
struct BranchQueryArg {
    branch: Option<String>,
}

// ---------- document content ----------

#[derive(Serialize)]
struct PublicDocContent {
    id: Uuid,
    path: String,
    title: String,
    content: String,
    version_id: Uuid,
    updated_at: DateTime<Utc>,
    size_bytes: i32,
}

/// Ensure the document belongs to the public project AND is published.
pub(crate) async fn published_doc(
    state: &AppState,
    proj: &project::Model,
    doc_id: Uuid,
) -> AppResult<document::Model> {
    document::Entity::find_by_id(doc_id)
        .one(&state.db)
        .await?
        .filter(|d| d.project_id == proj.id && d.published)
        .ok_or_else(|| AppError::NotFound("document not published".into()))
}

async fn get_doc_content(
    State(state): State<AppState>,
    Path((slug, doc_id)): Path<(String, Uuid)>,
    Query(q): Query<BranchQueryArg>,
) -> AppResult<Json<PublicDocContent>> {
    let proj = public_project(&state, &slug).await?;
    let br = default_branch(&state, &proj, q.branch.as_deref()).await?;
    let doc = published_doc(&state, &proj, doc_id).await?;

    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let st = vcs::doc_state(&state.db, &graph, br.head_commit_id, doc_id)
        .await?
        .filter(|s| s.change_type != "deleted")
        .ok_or_else(|| AppError::NotFound("document not on this branch".into()))?;
    let vid = st
        .version_id
        .ok_or_else(|| AppError::NotFound("document has no content".into()))?;
    let ver = doc_version::Entity::find_by_id(vid)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::Internal("version missing".into()))?;

    Ok(Json(PublicDocContent {
        id: doc.id,
        path: doc.path,
        title: doc.title,
        content: ver.content,
        version_id: ver.id,
        updated_at: st.updated_at,
        size_bytes: ver.size_bytes,
    }))
}

// ---------- link graph ----------

#[derive(Serialize)]
struct PublicLinkNode {
    id: Uuid,
    path: String,
    title: String,
    out: usize,
    inn: usize,
    dangling: usize,
}

#[derive(Serialize)]
struct PublicLinkEdge {
    source: Uuid,
    target: Option<Uuid>,
    raw_target: String,
    #[serde(rename = "type")]
    link_type: String,
    anchor: Option<String>,
}

#[derive(Serialize)]
struct PublicLinks {
    branch: PublicBranchItem,
    nodes: Vec<PublicLinkNode>,
    edges: Vec<PublicLinkEdge>,
}

async fn get_links(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(q): Query<BranchQueryArg>,
) -> AppResult<Json<PublicLinks>> {
    let proj = public_project(&state, &slug).await?;
    let br = default_branch(&state, &proj, q.branch.as_deref()).await?;
    let docs = published_docs(&state, &proj, br.head_commit_id).await?;
    let idx: HashMap<Uuid, usize> = docs.iter().enumerate().map(|(i, d)| (d.id, i)).collect();

    let edges = link_edge::Entity::find()
        .filter(link_edge::Column::BranchId.eq(br.id))
        .all(&state.db)
        .await?;
    let visible: Vec<_> = edges
        .into_iter()
        .filter(|e| idx.contains_key(&e.source_doc_id))
        .collect();

    let mut out = vec![0usize; docs.len()];
    let mut inn = vec![0usize; docs.len()];
    let mut dangling = vec![0usize; docs.len()];
    for e in &visible {
        if let Some(si) = idx.get(&e.source_doc_id) {
            out[*si] += 1;
            if e.target_doc_id.is_none() {
                dangling[*si] += 1;
            }
        }
        if let Some(ti) = e.target_doc_id.as_ref().and_then(|t| idx.get(t)) {
            inn[*ti] += 1;
        }
    }

    Ok(Json(PublicLinks {
        branch: PublicBranchItem {
            id: br.id,
            name: br.name,
            is_default: br.is_default,
        },
        nodes: docs
            .iter()
            .enumerate()
            .map(|(i, d)| PublicLinkNode {
                id: d.id,
                path: d.path.clone(),
                title: d.title.clone(),
                out: out[i],
                inn: inn[i],
                dangling: dangling[i],
            })
            .collect(),
        edges: visible
            .into_iter()
            .map(|e| PublicLinkEdge {
                source: e.source_doc_id,
                target: e.target_doc_id,
                raw_target: e.raw_target,
                link_type: e.link_type,
                anchor: e.anchor,
            })
            .collect(),
    }))
}

// ---------- history / diff (read-only) ----------

#[derive(Serialize)]
struct PublicRevision {
    commit_id: Uuid,
    message: String,
    commit_type: String,
    author: String,
    created_at: DateTime<Utc>,
    change_type: String,
    version_id: Option<Uuid>,
}

async fn doc_history(
    State(state): State<AppState>,
    Path((slug, doc_id)): Path<(String, Uuid)>,
    Query(q): Query<BranchQueryArg>,
) -> AppResult<Json<Vec<PublicRevision>>> {
    let proj = public_project(&state, &slug).await?;
    let br = default_branch(&state, &proj, q.branch.as_deref()).await?;
    published_doc(&state, &proj, doc_id).await?;

    let Some(head) = br.head_commit_id else {
        return Ok(Json(vec![]));
    };
    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let chain = graph.chain(head);
    let pos: HashMap<Uuid, usize> = chain.iter().enumerate().map(|(i, id)| (*id, i)).collect();

    let cds = commit_doc::Entity::find()
        .filter(commit_doc::Column::DocumentId.eq(doc_id))
        .filter(commit_doc::Column::CommitId.is_in(chain))
        .all(&state.db)
        .await?;
    if cds.is_empty() {
        return Ok(Json(vec![]));
    }

    let ids: Vec<Uuid> = cds.iter().map(|c| c.commit_id).collect();
    let commits = commit::Entity::find()
        .filter(commit::Column::Id.is_in(ids))
        .all(&state.db)
        .await?;
    let by_id: HashMap<Uuid, commit::Model> = commits.into_iter().map(|c| (c.id, c)).collect();

    let author_ids: Vec<Uuid> = by_id.values().map(|c| c.author_id).collect();
    let users = user::Entity::find()
        .filter(user::Column::Id.is_in(author_ids))
        .all(&state.db)
        .await?;
    let by_user: HashMap<Uuid, user::Model> = users.into_iter().map(|u| (u.id, u)).collect();

    let mut out: Vec<PublicRevision> = cds
        .into_iter()
        .map(|cd| {
            let c = by_id.get(&cd.commit_id);
            PublicRevision {
                commit_id: cd.commit_id,
                message: c.map(|c| c.message.clone()).unwrap_or_default(),
                commit_type: c.map(|c| c.commit_type.clone()).unwrap_or_default(),
                author: c
                    .and_then(|c| by_user.get(&c.author_id))
                    .map(owner_name)
                    .unwrap_or_default(),
                created_at: c.map(|c| c.created_at).unwrap_or_default(),
                change_type: cd.change_type.clone(),
                version_id: cd.version_id,
            }
        })
        .collect();
    out.sort_by_key(|r| pos.get(&r.commit_id).copied().unwrap_or(usize::MAX));
    Ok(Json(out))
}

#[derive(Deserialize)]
struct PublicDiffQuery {
    branch: Option<String>,
    from: Option<Uuid>,
    to: Option<Uuid>,
    context: Option<usize>,
}

#[derive(Serialize)]
struct PublicDiff {
    from_label: String,
    to_label: String,
    has_changes: bool,
    diff: String,
}

async fn content_at(
    state: &AppState,
    doc_id: Uuid,
    commit_id: Uuid,
) -> AppResult<Option<String>> {
    let cd = commit_doc::Entity::find()
        .filter(commit_doc::Column::DocumentId.eq(doc_id))
        .filter(commit_doc::Column::CommitId.eq(commit_id))
        .one(&state.db)
        .await?;
    let Some(cd) = cd else { return Ok(None) };
    let Some(vid) = cd.version_id else { return Ok(None) };
    Ok(doc_version::Entity::find_by_id(vid)
        .one(&state.db)
        .await?
        .map(|v| v.content))
}

async fn doc_diff(
    State(state): State<AppState>,
    Path((slug, doc_id)): Path<(String, Uuid)>,
    Query(q): Query<PublicDiffQuery>,
) -> AppResult<Json<PublicDiff>> {
    let proj = public_project(&state, &slug).await?;
    let br = default_branch(&state, &proj, q.branch.as_deref()).await?;
    published_doc(&state, &proj, doc_id).await?;

    let old_content = match q.from {
        Some(c) => content_at(&state, doc_id, c).await?.unwrap_or_default(),
        None => String::new(),
    };
    let new_content = match q.to {
        Some(c) => content_at(&state, doc_id, c).await?.unwrap_or_default(),
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
    let text = diff::unified(&old_content, &new_content, q.context.unwrap_or(3).min(20));
    Ok(Json(PublicDiff {
        from_label: q
            .from
            .map(|u| u.to_string().chars().take(8).collect())
            .unwrap_or_else(|| "start".into()),
        to_label: q
            .to
            .map(|u| u.to_string().chars().take(8).collect())
            .unwrap_or_else(|| "current".into()),
        has_changes: !text.is_empty(),
        diff: text,
    }))
}

// ---------- 全文检索（只查已发布文档）----------

#[derive(Deserialize)]
struct SearchQueryArg {
    q: String,
    branch: Option<String>,
    /// 最多返回多少条命中（默认 50，上限 200）
    limit: Option<usize>,
}

#[derive(Serialize)]
struct PublicSearch {
    branch: PublicBranchItem,
    query: String,
    /// 全部命中数，可能大于返回的条数
    total: usize,
    truncated: bool,
    hits: Vec<search::Hit>,
}

async fn search_docs(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(q): Query<SearchQueryArg>,
) -> AppResult<Json<PublicSearch>> {
    let proj = public_project(&state, &slug).await?;
    let br = default_branch(&state, &proj, q.branch.as_deref()).await?;

    let term = q.q.trim();
    if term.is_empty() {
        return Err(AppError::BadRequest("检索词不能为空".into()));
    }
    let limit = q.limit.unwrap_or(50).clamp(1, 200);

    let docs = published_docs(&state, &proj, br.head_commit_id).await?;
    let graph = vcs::CommitGraph::load(&state.db, proj.id).await?;
    let states = vcs::branch_doc_states(&state.db, &graph, br.head_commit_id).await?;
    let versions: HashMap<Uuid, Uuid> = states
        .values()
        .filter(|s| s.change_type != "deleted")
        .filter_map(|s| s.version_id.map(|v| (s.document_id, v)))
        .collect();

    let ver_ids: Vec<Uuid> = docs.iter().filter_map(|d| versions.get(&d.id).copied()).collect();
    let contents: HashMap<Uuid, String> = if ver_ids.is_empty() {
        HashMap::new()
    } else {
        doc_version::Entity::find()
            .filter(doc_version::Column::Id.is_in(ver_ids))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|v| (v.id, v.content))
            .collect()
    };

    // 按路径排序，结果顺序稳定
    let mut ordered = docs;
    ordered.sort_by(|a, b| a.path.cmp(&b.path));

    let mut texts: Vec<search::DocText> = Vec::new();
    for d in &ordered {
        let Some(vid) = versions.get(&d.id) else { continue };
        let Some(content) = contents.get(vid) else {
            continue;
        };
        texts.push(search::DocText {
            doc_id: d.id,
            path: &d.path,
            title: &d.title,
            content,
        });
    }
    let (total, hits, truncated) = search::run(texts, term, limit);

    Ok(Json(PublicSearch {
        branch: PublicBranchItem {
            id: br.id,
            name: br.name,
            is_default: br.is_default,
        },
        query: term.to_string(),
        total,
        truncated,
        hits,
    }))
}
