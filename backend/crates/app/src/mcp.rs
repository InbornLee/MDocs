//! MCP server exposing published documentation as an AI knowledge base.
//!
//! Served over the Streamable HTTP transport at `/mcp`. Access is read-only
//! and scoped exactly like the public site: only projects with
//! `visibility = "public"` and documents with `published = true` are visible,
//! so an AI client can never reach private content.

use std::collections::HashMap;

use rmcp::{
    handler::server::{
        tool::{Extension, ToolRouter},
        wrapper::Parameters,
    },
    model::{CallToolResult, ContentBlock},
    tool, tool_handler, tool_router, ErrorData as McpError,
};
use schemars::JsonSchema;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::routes::public as pubapi;
use crate::services::vcs;
use crate::state::AppState;

#[derive(Clone)]
pub struct DocsServer {
    state: AppState,
    tool_router: ToolRouter<Self>,
}

/// Map any error convertible into an `AppError` (including `DbErr`) onto MCP.
fn err<E: Into<crate::error::AppError>>(e: E) -> McpError {
    let e: crate::error::AppError = e.into();
    let msg = e.to_string();
    match e {
        crate::error::AppError::NotFound(m) => McpError::resource_not_found(m, None),
        crate::error::AppError::BadRequest(m) | crate::error::AppError::Conflict(m) => {
            McpError::invalid_params(m, None)
        }
        _ => McpError::internal_error(msg, None),
    }
}

fn to_result<T: Serialize>(v: &T) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string_pretty(v)
        .map_err(|e| McpError::internal_error(format!("serialize: {e}"), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

// ---------- tool parameters / outputs ----------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListProjectsArgs {
    /// Optional case-insensitive substring filter on name / slug / description.
    #[serde(default)]
    pub query: Option<String>,
    /// 1-based page number (default 1).
    #[serde(default)]
    pub page: Option<u64>,
    /// Page size, 1-50 (default 20).
    #[serde(default)]
    pub page_size: Option<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectSummary {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub owner: String,
    pub doc_count: usize,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectArgs {
    /// Project slug (as shown on the public homepage).
    pub project: String,
    /// Branch name; omit for the project's default branch.
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocSummary {
    pub id: String,
    pub path: String,
    pub title: String,
    pub size_bytes: i64,
    pub updated_at: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocList {
    pub project: String,
    pub branch: String,
    pub count: usize,
    pub documents: Vec<DocSummary>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadDocArgs {
    pub project: String,
    /// Document path, e.g. "docs/arch/db.md". Provide this or `doc_id`.
    #[serde(default)]
    pub path: Option<String>,
    /// Document id (UUID). Provide this or `path`.
    #[serde(default)]
    pub doc_id: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocContent {
    pub id: String,
    pub path: String,
    pub title: String,
    pub branch: String,
    pub content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchArgs {
    pub project: String,
    /// Substring to look for (case-insensitive) in titles and body text.
    pub query: String,
    /// Maximum matches to return, 1-50 (default 10).
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchHit {
    pub id: String,
    pub path: String,
    pub title: String,
    /// Short window of text around the first match.
    pub snippet: String,
    /// Number of occurrences within the document.
    pub occurrences: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchResults {
    pub project: String,
    pub query: String,
    pub count: usize,
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GraphNode {
    pub id: String,
    pub path: String,
    pub title: String,
    pub out_links: usize,
    pub in_links: usize,
    pub dangling: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GraphEdge {
    pub source: String,
    pub target: Option<String>,
    pub raw_target: String,
    pub link_type: String,
    pub anchor: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LinkGraph {
    pub project: String,
    pub branch: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub unreachable_count: usize,
}

// ---------- shared helpers ----------

/// 调用者身份：不带 token 只能看公开项目与已发布文档；
/// 带有效 token 时按该用户在该项目里的角色决定可见范围。
#[derive(Clone, Copy)]
enum Caller {
    Anon,
    User(Uuid),
}

/// 从 `Authorization: Bearer <token>` 解析调用者。
/// token 无效直接报错，不静默降级成匿名（否则用户会以为自己在用私有数据）。
async fn caller(state: &AppState, parts: &axum::http::request::Parts) -> Result<Caller, McpError> {
    let Some(raw) = parts.headers.get(axum::http::header::AUTHORIZATION) else {
        return Ok(Caller::Anon);
    };
    let text = raw
        .to_str()
        .map_err(|_| McpError::invalid_request("invalid Authorization header", None))?;
    let token = text
        .strip_prefix("Bearer ")
        .or_else(|| text.strip_prefix("bearer "))
        .ok_or_else(|| McpError::invalid_request("Authorization must be `Bearer <token>`", None))?;

    match crate::services::api_token::resolve(&state.db, token).await {
        Ok(Some(uid)) => {
            // 被禁用的账号连 API token 也一并失效
            let u = crate::services::auth::load_user(&state.db, uid).await.map_err(err)?;
            if !u.enabled {
                return Err(McpError::invalid_request("account disabled", None));
            }
            Ok(Caller::User(uid))
        }
        Ok(None) => Err(McpError::invalid_request(
            "invalid API token (create one in Settings → API token)",
            None,
        )),
        Err(e) => Err(err(e)),
    }
}

/// 解析项目 + 分支，并判断调用者能否看到未发布文档（项目成员可以）。
async fn scope(
    state: &AppState,
    project: &str,
    branch: Option<&str>,
    caller: Caller,
) -> Result<(
    crate::entities::project::Model,
    crate::entities::branch::Model,
    bool,
), McpError> {
    let slug = project.trim();
    let proj = crate::services::perm::get_project(&state.db, slug)
        .await
        .map_err(err)?;
    let role = match caller {
        Caller::Anon => {
            if proj.visibility == "public" {
                crate::services::perm::Role::Public
            } else {
                crate::services::perm::Role::None
            }
        }
        Caller::User(id) => crate::services::perm::project_role(&state.db, &proj, id)
            .await
            .map_err(err)?,
    };
    if role < crate::services::perm::Role::Public {
        return Err(McpError::resource_not_found(
            format!("project '{slug}' not found"),
            None,
        ));
    }
    let br = pubapi::default_branch(state, &proj, branch).await.map_err(err)?;
    Ok((proj, br, role >= crate::services::perm::Role::Reader))
}

/// 分支上的文档；`with_unpublished` 为 true（项目成员）时包含未发布文档。
async fn branch_docs(
    state: &AppState,
    proj: &crate::entities::project::Model,
    br: &crate::entities::branch::Model,
    with_unpublished: bool,
) -> Result<Vec<crate::entities::document::Model>, McpError> {
    if !with_unpublished {
        return pubapi::published_docs(state, proj, br.head_commit_id)
            .await
            .map_err(err);
    }

    let graph = vcs::CommitGraph::load(&state.db, proj.id).await.map_err(err)?;
    let states = vcs::branch_doc_states(&state.db, &graph, br.head_commit_id)
        .await
        .map_err(err)?;
    let alive: Vec<Uuid> = states
        .values()
        .filter(|s| s.change_type != "deleted")
        .map(|s| s.document_id)
        .collect();
    if alive.is_empty() {
        return Ok(vec![]);
    }
    crate::entities::document::Entity::find()
        .filter(crate::entities::document::Column::ProjectId.eq(proj.id))
        .filter(crate::entities::document::Column::Id.is_in(alive))
        .all(&state.db)
        .await
        .map_err(err)
}

/// 分支上的文档及其当前内容。
async fn docs_with_content(
    state: &AppState,
    proj: &crate::entities::project::Model,
    br: &crate::entities::branch::Model,
    with_unpublished: bool,
) -> Result<Vec<(crate::entities::document::Model, String)>, McpError> {
    let docs = branch_docs(state, proj, br, with_unpublished).await?;
    let graph = vcs::CommitGraph::load(&state.db, proj.id).await.map_err(err)?;
    let states = vcs::branch_doc_states(&state.db, &graph, br.head_commit_id)
        .await
        .map_err(err)?;

    let mut out = Vec::with_capacity(docs.len());
    for d in docs {
        let vid = states.get(&d.id).and_then(|s| s.version_id);
        let content = match vid {
            Some(vid) => crate::entities::doc_version::Entity::find_by_id(vid)
                .one(&state.db)
                .await
                .map_err(crate::error::AppError::from)
                .map_err(err)?
                .map(|v| v.content)
                .unwrap_or_default(),
            None => String::new(),
        };
        out.push((d, content));
    }
    Ok(out)
}

fn snippet(content: &str, query: &str) -> (String, usize) {
    let lower = content.to_lowercase();
    let needle = query.to_lowercase();
    let count = lower.matches(&needle).count();
    let Some(idx) = lower.find(&needle) else {
        return (String::new(), 0);
    };
    let start = idx.saturating_sub(80);
    let end = (idx + needle.len() + 120).min(content.len());
    // snap to char boundaries
    let mut s = start;
    while s < content.len() && !content.is_char_boundary(s) {
        s += 1;
    }
    let mut e = end;
    while e < content.len() && !content.is_char_boundary(e) {
        e += 1;
    }
    let mut out = String::new();
    if s > 0 {
        out.push('…');
    }
    out.push_str(content[s..e].trim());
    if e < content.len() {
        out.push('…');
    }
    (out.replace('\n', " "), count)
}

fn ts(t: chrono::DateTime<chrono::Utc>) -> String {
    t.to_rfc3339()
}

// ---------- tools ----------

#[tool_router]
impl DocsServer {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    /// List documentation projects available as knowledge bases.
    #[tool(
        name = "list_projects",
        description = "List documentation projects. Without an API token only public projects are returned; pass `Authorization: Bearer <token>` (create one in Settings → API token) to also see the projects you are a member of."
    )]
    async fn list_projects(
        &self,
        Parameters(args): Parameters<ListProjectsArgs>,
        Extension(parts): Extension<axum::http::request::Parts>,
    ) -> Result<CallToolResult, McpError> {
        let caller = caller(&self.state, &parts).await?;
        let page = args.page.unwrap_or(1).max(1);
        let page_size = args.page_size.unwrap_or(20).clamp(1, 50);

        let mut visible = sea_orm::Condition::any()
            .add(crate::entities::project::Column::Visibility.eq("public"));
        if let Caller::User(uid) = caller {
            let member_of: Vec<Uuid> = crate::entities::project_member::Entity::find()
                .filter(crate::entities::project_member::Column::UserId.eq(uid))
                .all(&self.state.db)
                .await
                .map_err(err)?
                .into_iter()
                .map(|m| m.project_id)
                .collect();
            visible = visible.add(crate::entities::project::Column::OwnerId.eq(uid));
            if !member_of.is_empty() {
                visible = visible.add(crate::entities::project::Column::Id.is_in(member_of));
            }
        }

        let mut cond = sea_orm::Condition::all().add(visible);
        if let Some(kw) = args.query.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let like = format!("%{kw}%");
            cond = cond.add(
                sea_orm::Condition::any()
                    .add(crate::entities::project::Column::Name.like(&like))
                    .add(crate::entities::project::Column::Slug.like(&like))
                    .add(crate::entities::project::Column::Description.like(&like)),
            );
        }

        let total = crate::entities::project::Entity::find()
            .filter(cond.clone())
            .count(&self.state.db)
            .await
            .map_err(crate::error::AppError::from)
            .map_err(err)? as usize;

        let rows = crate::entities::project::Entity::find()
            .filter(cond)
            .order_by_desc(crate::entities::project::Column::UpdatedAt)
            .paginate(&self.state.db, page_size)
            .fetch_page(page - 1)
            .await
            .map_err(crate::error::AppError::from)
            .map_err(err)?;

        let mut projects = Vec::with_capacity(rows.len());
        for p in rows {
            let role = match caller {
                Caller::Anon => crate::services::perm::Role::Public,
                Caller::User(id) => crate::services::perm::project_role(&self.state.db, &p, id)
                    .await
                    .map_err(err)?,
            };
            if role < crate::services::perm::Role::Public {
                continue;
            }
            let br = match p.default_branch_id {
                Some(bid) => crate::entities::branch::Entity::find_by_id(bid)
                    .one(&self.state.db)
                    .await
                    .map_err(crate::error::AppError::from)
                    .map_err(err)?,
                None => None,
            };
            let doc_count = match br {
                Some(br) => branch_docs(
                    &self.state,
                    &p,
                    &br,
                    role >= crate::services::perm::Role::Reader,
                )
                .await?
                .len(),
                None => 0,
            };
            projects.push(ProjectSummary {
                slug: p.slug.clone(),
                name: p.name.clone(),
                description: p.description.clone(),
                owner: pubapi::display_name(&self.state, p.owner_id)
                    .await
                    .map_err(err)?,
                doc_count,
                updated_at: ts(p.updated_at),
            });
        }

        to_result(&serde_json::json!({
            "total": total,
            "page": page,
            "page_size": page_size,
            "projects": projects,
        }))
    }

    /// List the documents of a project (the knowledge base index).
    #[tool(
        name = "list_documents",
        description = "List the documents of a project. Returns path, title and size for each document so you can pick what to read."
    )]
    async fn list_documents(
        &self,
        Parameters(args): Parameters<ProjectArgs>,
        Extension(parts): Extension<axum::http::request::Parts>,
    ) -> Result<CallToolResult, McpError> {
        let caller = caller(&self.state, &parts).await?;
        let (proj, br, with_unpublished) =
            scope(&self.state, &args.project, args.branch.as_deref(), caller).await?;
        let docs = branch_docs(&self.state, &proj, &br, with_unpublished).await?;
        let graph = vcs::CommitGraph::load(&self.state.db, proj.id)
            .await
            .map_err(crate::error::AppError::from)
            .map_err(err)?;
        let states = vcs::branch_doc_states(&self.state.db, &graph, br.head_commit_id)
            .await
            .map_err(crate::error::AppError::from)
            .map_err(err)?;

        let ver_ids: Vec<Uuid> = states.values().filter_map(|s| s.version_id).collect();
        let sizes: HashMap<Uuid, i32> = if ver_ids.is_empty() {
            Default::default()
        } else {
            crate::entities::doc_version::Entity::find()
                .filter(crate::entities::doc_version::Column::Id.is_in(ver_ids))
                .all(&self.state.db)
                .await
                .map_err(crate::error::AppError::from)
                .map_err(err)?
                .into_iter()
                .map(|v| (v.id, v.size_bytes))
                .collect()
        };

        let mut documents: Vec<DocSummary> = docs
            .iter()
            .map(|d| DocSummary {
                id: d.id.to_string(),
                path: d.path.clone(),
                title: d.title.clone(),
                size_bytes: states
                    .get(&d.id)
                    .and_then(|s| s.version_id)
                    .and_then(|v| sizes.get(&v).copied())
                    .unwrap_or(0) as i64,
                updated_at: states
                    .get(&d.id)
                    .map(|s| ts(s.updated_at))
                    .unwrap_or_else(|| ts(d.updated_at)),
            })
            .collect();
        documents.sort_by(|a, b| a.path.cmp(&b.path));

        to_result(&DocList {
            project: proj.slug,
            branch: br.name,
            count: documents.len(),
            documents,
        })
    }

    /// Read one document in full.
    #[tool(
        name = "read_document",
        description = "Read the full Markdown content of one document, addressed by path or document id."
    )]
    async fn read_document(
        &self,
        Parameters(args): Parameters<ReadDocArgs>,
        Extension(parts): Extension<axum::http::request::Parts>,
    ) -> Result<CallToolResult, McpError> {
        if args.path.is_none() && args.doc_id.is_none() {
            return Err(McpError::invalid_params(
                "provide either `path` or `doc_id`",
                None,
            ));
        }
        let caller = caller(&self.state, &parts).await?;
        let (proj, br, with_unpublished) =
            scope(&self.state, &args.project, args.branch.as_deref(), caller).await?;
        let docs = branch_docs(&self.state, &proj, &br, with_unpublished).await?;

        let wanted_id = match args.doc_id.as_deref() {
            Some(s) => Some(
                Uuid::parse_str(s.trim())
                    .map_err(|_| McpError::invalid_params("doc_id must be a UUID", None))?,
            ),
            None => None,
        };
        let want_path = args
            .path
            .as_deref()
            .map(|p| p.trim().trim_start_matches("./").to_string());

        let doc = docs
            .iter()
            .find(|d| {
                wanted_id.is_some_and(|id| d.id == id)
                    || want_path.as_deref().is_some_and(|p| d.path == p)
                    // tolerate a missing .md extension
                    || want_path
                        .as_deref()
                        .is_some_and(|p| format!("{p}.md") == d.path)
            })
            .ok_or_else(|| {
                McpError::resource_not_found(
                    format!("no document matches in project '{}'", args.project),
                    None,
                )
            })?;

        let ver = {
            let graph = vcs::CommitGraph::load(&self.state.db, proj.id)
                .await
                .map_err(crate::error::AppError::from)
                .map_err(err)?;
            let st = vcs::doc_state(&self.state.db, &graph, br.head_commit_id, doc.id)
                .await
                .map_err(crate::error::AppError::from)
                .map_err(err)?;
            match st.and_then(|s| s.version_id) {
                Some(vid) => crate::entities::doc_version::Entity::find_by_id(vid)
                    .one(&self.state.db)
                    .await
                    .map_err(crate::error::AppError::from)
                    .map_err(err)?,
                None => None,
            }
        };
        let content = ver
            .ok_or_else(|| McpError::resource_not_found("document has no content", None))?
            .content;

        to_result(&DocContent {
            id: doc.id.to_string(),
            path: doc.path.clone(),
            title: doc.title.clone(),
            branch: br.name,
            content,
        })
    }

    /// Full-text search across the documents of a project.
    #[tool(
        name = "search_documents",
        description = "Search the documents of a project by keyword and return matching documents with a text snippet."
    )]
    async fn search_documents(
        &self,
        Parameters(args): Parameters<SearchArgs>,
        Extension(parts): Extension<axum::http::request::Parts>,
    ) -> Result<CallToolResult, McpError> {
        let q = args.query.trim();
        if q.is_empty() {
            return Err(McpError::invalid_params("query must not be empty", None));
        }
        let limit = args.limit.unwrap_or(10).clamp(1, 50);
        let caller = caller(&self.state, &parts).await?;
        let (proj, br, with_unpublished) =
            scope(&self.state, &args.project, args.branch.as_deref(), caller).await?;
        let docs = docs_with_content(&self.state, &proj, &br, with_unpublished).await?;
        let needle = q.to_lowercase();

        let mut hits: Vec<SearchHit> = Vec::new();
        for (d, content) in &docs {
            let in_title = d.title.to_lowercase().contains(&needle)
                || d.path.to_lowercase().contains(&needle);
            let (sn, count) = snippet(content, q);
            if in_title || count > 0 {
                hits.push(SearchHit {
                    id: d.id.to_string(),
                    path: d.path.clone(),
                    title: d.title.clone(),
                    snippet: if sn.is_empty() {
                        content.chars().take(160).collect::<String>().replace('\n', " ")
                    } else {
                        sn
                    },
                    occurrences: count,
                });
            }
        }
        hits.sort_by(|a, b| b.occurrences.cmp(&a.occurrences).then(a.path.cmp(&b.path)));
        let total = hits.len();
        hits.truncate(limit);

        to_result(&SearchResults {
            project: proj.slug,
            query: q.to_string(),
            count: total,
            hits,
        })
    }

    /// Link graph between the documents (the knowledge graph).
    #[tool(
        name = "get_link_graph",
        description = "Return the directed link graph between the documents of a project, useful for navigating related documents."
    )]
    async fn get_link_graph(
        &self,
        Parameters(args): Parameters<ProjectArgs>,
        Extension(parts): Extension<axum::http::request::Parts>,
    ) -> Result<CallToolResult, McpError> {
        let caller = caller(&self.state, &parts).await?;
        let (proj, br, with_unpublished) =
            scope(&self.state, &args.project, args.branch.as_deref(), caller).await?;
        let docs = branch_docs(&self.state, &proj, &br, with_unpublished).await?;
        let idx: HashMap<Uuid, usize> = docs.iter().enumerate().map(|(i, d)| (d.id, i)).collect();

        let edges_db = crate::entities::link_edge::Entity::find()
            .filter(crate::entities::link_edge::Column::BranchId.eq(br.id))
            .all(&self.state.db)
            .await
            .map_err(crate::error::AppError::from)
            .map_err(err)?;
        let visible: Vec<_> = edges_db
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

        let unreachable_count = docs
            .iter()
            .enumerate()
            .filter(|(i, _)| out[*i] == 0 && inn[*i] == 0)
            .count();

        to_result(&LinkGraph {
            project: proj.slug,
            branch: br.name,
            nodes: docs
                .iter()
                .enumerate()
                .map(|(i, d)| GraphNode {
                    id: d.id.to_string(),
                    path: d.path.clone(),
                    title: d.title.clone(),
                    out_links: out[i],
                    in_links: inn[i],
                    dangling: dangling[i],
                })
                .collect(),
            edges: visible
                .into_iter()
                .map(|e| GraphEdge {
                    source: e.source_doc_id.to_string(),
                    target: e.target_doc_id.map(|t| t.to_string()),
                    raw_target: e.raw_target,
                    link_type: e.link_type,
                    anchor: e.anchor,
                })
                .collect(),
            unreachable_count,
        })
    }
}

/// `#[tool_handler]` fills in `get_info` / `list_tools` / `call_tool`.
#[tool_handler(
    name = "mdocs",
    instructions = "Read-only access to Markdown documentation. Start with `list_projects`, then `list_documents` to see the index, `search_documents` to find relevant material, `read_document` to read a document in full, and `get_link_graph` to explore how documents reference each other. Without credentials only public projects and published documents are visible; send an API token (Settings → API token) as `Authorization: Bearer <token>` to also reach the projects you are a member of, including their unpublished documents."
)]
impl rmcp::ServerHandler for DocsServer {}

/// Build the Streamable HTTP service mounted at `/mcp`.
pub fn service(
    state: AppState,
) -> rmcp::transport::streamable_http_server::StreamableHttpService<
    DocsServer,
    rmcp::transport::streamable_http_server::session::local::LocalSessionManager,
> {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };
    let mut cfg = StreamableHttpServerConfig::default();
    cfg.allowed_hosts = state.cfg.mcp_allowed_hosts();
    StreamableHttpService::new(
        move || Ok(DocsServer::new(state.clone())),
        std::sync::Arc::new(LocalSessionManager::default()),
        cfg,
    )
}
