//! MCP server exposing documentation as an AI knowledge base.
//!
//! Served over the Streamable HTTP transport at `/mcp`. Access is read-only and
//! scoped exactly like the public site: only projects with `visibility =
//! "public"` and documents with `published = true` are visible to anonymous
//! callers. A valid `Authorization: Bearer mdocs_...` token widens the scope to
//! every project the token owner is a member of, including unpublished drafts.
//!
//! Besides tools, the server exposes MCP resources (`mdocs://{project}/{path}`)
//! so clients can pull documents directly, and a citation-oriented prompt.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::{extract::ConnectInfo, http::request::Parts};
use rmcp::{
    handler::server::{
        tool::{Extension, ToolCallContext, ToolRouter},
        wrapper::Parameters,
    },
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock,
        GetPromptRequestParams, GetPromptResponse, ListPromptsResult, ListResourceTemplatesResult,
        ListResourcesResult, PaginatedRequestParams, Prompt, PromptArgument, PromptMessage,
        ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource,
        ResourceContents, ResourceTemplate, Role,
    },
    service::RequestContext,
    tool, tool_handler, tool_router, ErrorData as McpError, RoleServer,
};
use schemars::JsonSchema;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{branch, document, project, project_member};
use crate::services::{
    audit, heading,
    perm::{self, Role as ProjectRole},
    search, snapshot, vcs,
};
use crate::state::AppState;

#[derive(Clone)]
pub struct DocsServer {
    state: AppState,
    #[allow(dead_code)] // 宏生成的 `call_tool` 会读取它
    tool_router: ToolRouter<Self>,
}

/// Map any error convertible into an `AppError` (including `DbErr`) onto MCP.
fn err<E: Into<crate::error::AppError>>(e: E) -> McpError {
    let e: crate::error::AppError = e.into();
    let msg = e.to_string();
    match e {
        crate::error::AppError::NotFound(m) => McpError::resource_not_found(m, None),
        crate::error::AppError::Unauthorized(m) => McpError::invalid_request(m, None),
        crate::error::AppError::Forbidden(m) => McpError::invalid_params(m, None),
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
    /// Default branch name, if the project has one.
    pub default_branch: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectArgs {
    /// Project slug (as shown on the public homepage).
    pub project: String,
    /// Branch name; omit for the project's default branch.
    #[serde(default)]
    pub branch: Option<String>,
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
    /// Read only one section: a heading slug (e.g. "installation") or its title.
    #[serde(default)]
    pub section: Option<String>,
    /// 1-based first line to return (used when `section` is absent).
    #[serde(default)]
    pub offset: Option<usize>,
    /// Maximum number of lines to return (1-5000, default 200 with offset).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocSummary {
    pub id: String,
    pub path: String,
    pub title: String,
    pub size_bytes: i64,
    pub updated_at: String,
    /// Link to the reading view (absolute when APP_PUBLIC_BASE_URL is set).
    pub url: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocList {
    pub project: String,
    pub branch: String,
    pub count: usize,
    pub documents: Vec<DocSummary>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocContent {
    pub id: String,
    pub path: String,
    pub title: String,
    pub branch: String,
    pub url: String,
    pub commit_id: Option<String>,
    pub updated_at: String,
    pub content: String,
    /// 1-based line of the first returned line.
    pub line_start: usize,
    /// 1-based line of the last returned line.
    pub line_end: usize,
    pub total_lines: usize,
    pub total_bytes: usize,
    /// True when the response was cut (byte cap or `limit`).
    pub truncated: bool,
    /// Heading title when `section` was used.
    pub section: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct OutlineHeading {
    pub level: u8,
    pub title: String,
    /// Anchor used by the reading view (pass it to `read_document.section`).
    pub slug: String,
    pub line: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocOutline {
    pub id: String,
    pub path: String,
    pub title: String,
    pub branch: String,
    pub url: String,
    pub total_lines: usize,
    pub total_bytes: usize,
    pub headings: Vec<OutlineHeading>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// Single project slug. Provide this or `projects`.
    #[serde(default)]
    pub project: Option<String>,
    /// Search several projects at once (merged and ranked together).
    #[serde(default)]
    pub projects: Option<Vec<String>>,
    /// Substring to look for (case-insensitive) in titles and body text.
    pub query: String,
    /// Maximum matches to return, 1-50 (default 10).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Skip the first N ranked hits (default 0).
    #[serde(default)]
    pub offset: Option<usize>,
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchHit {
    pub project: String,
    pub id: String,
    pub path: String,
    pub title: String,
    /// Reading-view link; append `#anchor` when `anchor` is present.
    pub url: String,
    /// Heading slug of the section containing the first match.
    pub anchor: Option<String>,
    pub commit_id: Option<String>,
    pub updated_at: String,
    /// Relevance score used for ordering (higher is better).
    pub score: f64,
    /// Short window of text around the first match.
    pub snippet: String,
    /// Number of occurrences within the document.
    pub occurrences: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchResults {
    pub projects: Vec<String>,
    pub query: String,
    /// Total ranked hits before pagination.
    pub count: usize,
    pub offset: usize,
    pub returned: usize,
    pub truncated: bool,
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BranchSummary {
    pub name: String,
    pub is_default: bool,
    pub head_commit_id: Option<String>,
    pub created_at: String,
    pub doc_count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BranchList {
    pub project: String,
    pub branches: Vec<BranchSummary>,
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

// ---------- caller / scope ----------

/// 调用者身份：不带 token 只能看公开项目与已发布文档；
/// 带有效 token 时按该用户在该项目里的角色决定可见范围。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Caller {
    Anon,
    User(Uuid),
}

#[derive(Clone, Debug)]
struct CallerInfo {
    user_id: Option<Uuid>,
    token_prefix: Option<String>,
    client_ip: Option<String>,
}

/// `call_tool` 统一完成鉴权后放进请求扩展，工具只消费结果，避免重复查 token。
#[derive(Clone, Debug)]
struct ResolvedCaller {
    caller: Caller,
    info: CallerInfo,
}

fn client_ip(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            parts
                .headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.trim().to_string())
        })
        // 直连（没有反向代理头）时用 TCP 连接信息兜底
        .or_else(|| {
            parts
                .extensions
                .get::<ConnectInfo<std::net::SocketAddr>>()
                .map(|ci| ci.0.ip().to_string())
        })
}

/// 从 `Authorization: Bearer <token>` 解析调用者。
/// token 无效直接报错，不静默降级成匿名（否则用户会以为自己在用私有数据）。
async fn caller(state: &AppState, parts: &Parts) -> Result<(Caller, CallerInfo), McpError> {
    let ip = client_ip(parts);
    let Some(raw) = parts.headers.get(axum::http::header::AUTHORIZATION) else {
        return Ok((
            Caller::Anon,
            CallerInfo {
                user_id: None,
                token_prefix: None,
                client_ip: ip,
            },
        ));
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
            let u = crate::services::auth::load_user(&state.db, uid)
                .await
                .map_err(err)?;
            if !u.enabled {
                return Err(McpError::invalid_request("account disabled", None));
            }
            Ok((
                Caller::User(uid),
                CallerInfo {
                    user_id: Some(uid),
                    token_prefix: Some(crate::services::api_token::display_prefix(token)),
                    client_ip: ip,
                },
            ))
        }
        Ok(None) => Err(McpError::invalid_request(
            "invalid API token (create one in Settings → API token)",
            None,
        )),
        Err(e) => Err(err(e)),
    }
}

fn parts_of(context: &RequestContext<RoleServer>) -> Option<&Parts> {
    context.extensions.get::<Parts>()
}

struct Scope {
    proj: project::Model,
    br: branch::Model,
    /// 项目成员（Reader 及以上）可以看到未发布文档
    with_unpublished: bool,
}

/// 解析项目 + 分支，并判断调用者能否看到未发布文档（项目成员可以）。
async fn scope(
    state: &AppState,
    project_slug: &str,
    branch_name: Option<&str>,
    caller: Caller,
) -> Result<Scope, McpError> {
    let slug = project_slug.trim();
    let proj = perm::get_project(&state.db, slug).await.map_err(err)?;
    let role = match caller {
        Caller::Anon => {
            if proj.visibility == "public" {
                ProjectRole::Public
            } else {
                ProjectRole::None
            }
        }
        Caller::User(id) => perm::project_role(&state.db, &proj, id)
            .await
            .map_err(err)?,
    };
    if role < ProjectRole::Public {
        return Err(McpError::resource_not_found(
            format!("project '{slug}' not found"),
            None,
        ));
    }
    let br = match branch_name {
        Some(_) => vcs::resolve_branch(&state.db, &proj, branch_name)
            .await
            .map_err(err)?,
        None => vcs::resolve_branch(&state.db, &proj, None)
            .await
            .map_err(err)?,
    };
    Ok(Scope {
        proj,
        br,
        with_unpublished: role >= ProjectRole::Reader,
    })
}

async fn load_snapshot(
    state: &AppState,
    scope: &Scope,
) -> Result<Arc<snapshot::Snapshot>, McpError> {
    snapshot::load(state, scope.proj.id, scope.br.id, scope.br.head_commit_id)
        .await
        .map_err(err)
}

/// 只统计文档数量，不加载全文（列表页用，避免为计数拉取整个知识库）。
async fn count_docs(
    state: &AppState,
    project_id: Uuid,
    head: Option<Uuid>,
    published_only: bool,
) -> Result<usize, McpError> {
    let graph = vcs::CommitGraph::load(&state.db, project_id)
        .await
        .map_err(crate::error::AppError::from)
        .map_err(err)?;
    let states = vcs::branch_doc_states(&state.db, &graph, head)
        .await
        .map_err(crate::error::AppError::from)
        .map_err(err)?;
    let alive: Vec<Uuid> = states
        .values()
        .filter(|s| s.change_type != "deleted")
        .map(|s| s.document_id)
        .collect();
    if alive.is_empty() {
        return Ok(0);
    }
    let mut q = document::Entity::find()
        .filter(document::Column::ProjectId.eq(project_id))
        .filter(document::Column::Id.is_in(alive));
    if published_only {
        q = q.filter(document::Column::Published.eq(true));
    }
    Ok(q.count(&state.db)
        .await
        .map_err(crate::error::AppError::from)
        .map_err(err)? as usize)
}

/// 当前调用者在项目中能看到的文档 id 集合。
async fn visible_ids(
    state: &AppState,
    scope: &Scope,
    snap: &snapshot::Snapshot,
) -> Result<HashSet<Uuid>, McpError> {
    if scope.with_unpublished {
        return Ok(snap.docs.iter().map(|d| d.id).collect());
    }
    let published = snapshot::published_ids(state, scope.proj.id)
        .await
        .map_err(err)?;
    Ok(snap
        .docs
        .iter()
        .filter(|d| published.contains(&d.id))
        .map(|d| d.id)
        .collect())
}

fn doc_url(state: &AppState, slug: &str, path: &str, internal: bool) -> String {
    let prefix = if internal { "/p/" } else { "/pub/" };
    let rel = format!("{prefix}{slug}/view/{}", encode_path(path));
    match &state.cfg.public_base_url {
        Some(base) if !base.trim().is_empty() => {
            format!("{}{}", base.trim().trim_end_matches('/'), rel)
        }
        _ => rel,
    }
}

fn encode_path(path: &str) -> String {
    use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
    // 保留 `/`，只编码会破坏 URL 的字符
    const SET: &AsciiSet = &CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b'#')
        .add(b'?')
        .add(b'%')
        .add(b'<')
        .add(b'>')
        .add(b'\\')
        .add(b'^')
        .add(b'`')
        .add(b'{')
        .add(b'}')
        .add(b'|');
    utf8_percent_encode(path, SET).to_string()
}

fn decode_path(path: &str) -> String {
    percent_encoding::percent_decode_str(path)
        .decode_utf8_lossy()
        .to_string()
}

fn audit_call(
    state: &AppState,
    info: &CallerInfo,
    tool: &str,
    project: Option<String>,
    detail: Option<String>,
) {
    audit::record(
        state,
        audit::AuditEntry {
            user_id: info.user_id,
            token_prefix: info.token_prefix.clone(),
            client_ip: info.client_ip.clone(),
            tool: tool.to_string(),
            project,
            detail,
            ok: true,
            error: None,
        },
    );
}

/// 一次工具调用的审计记录。
///
/// `call_tool` 在调用前后各执行一次：`finish` 写入真实结果；如果实现 panic
/// 或提前返回，`Drop` 兜底写一条失败记录，保证不会静默丢失。
struct ToolAudit<'a> {
    state: &'a AppState,
    info: CallerInfo,
    tool: String,
    project: Option<String>,
    detail: Option<String>,
    written: bool,
}

impl<'a> ToolAudit<'a> {
    fn new(
        state: &'a AppState,
        info: CallerInfo,
        tool: impl Into<String>,
        project: Option<String>,
        detail: Option<String>,
    ) -> Self {
        Self {
            state,
            info,
            tool: tool.into(),
            project,
            detail,
            written: false,
        }
    }

    fn write(&mut self, ok: bool, error: Option<String>) {
        if self.written {
            return;
        }
        self.written = true;
        audit::record(
            self.state,
            audit::AuditEntry {
                user_id: self.info.user_id,
                token_prefix: self.info.token_prefix.clone(),
                client_ip: self.info.client_ip.clone(),
                tool: self.tool.clone(),
                project: self.project.clone(),
                detail: self.detail.clone(),
                ok,
                error,
            },
        );
    }

    fn finish<T>(mut self, result: &Result<T, McpError>) {
        match result {
            Ok(_) => self.write(true, None),
            Err(e) => self.write(false, Some(e.message.to_string())),
        }
    }
}

impl Drop for ToolAudit<'_> {
    fn drop(&mut self) {
        if !self.written {
            let panicking = std::thread::panicking();
            self.write(!panicking, panicking.then(|| "tool panicked".to_string()));
        }
    }
}

/// 从工具参数里提取项目 slug（跨项目检索时取前几个）。
fn audit_project(args: &Option<serde_json::Map<String, serde_json::Value>>) -> Option<String> {
    let a = args.as_ref()?;
    if let Some(p) = a.get("project").and_then(|v| v.as_str()) {
        if !p.trim().is_empty() {
            return Some(p.to_string());
        }
    }
    a.get("projects")
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|v| v.as_str())
                .take(10)
                .collect::<Vec<_>>()
                .join(",")
        })
        .filter(|s| !s.is_empty())
}

/// 从工具参数里提取一小段可读摘要，不落文档正文。
fn audit_detail(args: &Option<serde_json::Map<String, serde_json::Value>>) -> Option<String> {
    let a = args.as_ref()?;
    let mut out = Vec::new();
    for key in [
        "query", "path", "section", "doc_id", "branch", "offset", "limit",
    ] {
        match a.get(key) {
            Some(serde_json::Value::String(s)) if !s.is_empty() => out.push(format!("{key}={s}")),
            Some(serde_json::Value::Number(n)) => out.push(format!("{key}={n}")),
            _ => {}
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join(" "))
    }
}

fn ts(t: chrono::DateTime<chrono::Utc>) -> String {
    t.to_rfc3339()
}

fn snippet_at(content: &str, term: &str, idx: usize) -> String {
    let start = idx.saturating_sub(160);
    let end = (idx + term.len() + 240).min(content.len());
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
    out.replace('\n', " ")
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
        Extension(resolved): Extension<ResolvedCaller>,
    ) -> Result<CallToolResult, McpError> {
        let caller = resolved.caller;
        let page = args.page.unwrap_or(1).max(1);
        let page_size = args.page_size.unwrap_or(20).clamp(1, 50);

        let mut visible = sea_orm::Condition::any().add(project::Column::Visibility.eq("public"));
        if let Caller::User(uid) = caller {
            let member_of: Vec<Uuid> = project_member::Entity::find()
                .filter(project_member::Column::UserId.eq(uid))
                .all(&self.state.db)
                .await
                .map_err(err)?
                .into_iter()
                .map(|m| m.project_id)
                .collect();
            visible = visible.add(project::Column::OwnerId.eq(uid));
            if !member_of.is_empty() {
                visible = visible.add(project::Column::Id.is_in(member_of));
            }
        }

        let mut cond = sea_orm::Condition::all().add(visible);
        if let Some(kw) = args
            .query
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let like = format!("%{kw}%");
            cond = cond.add(
                sea_orm::Condition::any()
                    .add(project::Column::Name.like(&like))
                    .add(project::Column::Slug.like(&like))
                    .add(project::Column::Description.like(&like)),
            );
        }

        let total = project::Entity::find()
            .filter(cond.clone())
            .count(&self.state.db)
            .await
            .map_err(crate::error::AppError::from)
            .map_err(err)? as usize;

        let rows = project::Entity::find()
            .filter(cond)
            .order_by_desc(project::Column::UpdatedAt)
            .paginate(&self.state.db, page_size)
            .fetch_page(page - 1)
            .await
            .map_err(crate::error::AppError::from)
            .map_err(err)?;

        let mut projects = Vec::with_capacity(rows.len());
        for p in rows {
            let role = match caller {
                Caller::Anon => ProjectRole::Public,
                Caller::User(id) => perm::project_role(&self.state.db, &p, id)
                    .await
                    .map_err(err)?,
            };
            if role < ProjectRole::Public {
                continue;
            }
            let br = match p.default_branch_id {
                Some(bid) => branch::Entity::find_by_id(bid)
                    .one(&self.state.db)
                    .await
                    .map_err(crate::error::AppError::from)
                    .map_err(err)?,
                None => None,
            };
            let doc_count = match &br {
                Some(br) => {
                    count_docs(
                        &self.state,
                        p.id,
                        br.head_commit_id,
                        role < ProjectRole::Reader,
                    )
                    .await?
                }
                None => 0,
            };
            projects.push(ProjectSummary {
                slug: p.slug.clone(),
                name: p.name.clone(),
                description: p.description.clone(),
                owner: crate::routes::public::display_name(&self.state, p.owner_id)
                    .await
                    .map_err(err)?,
                doc_count,
                updated_at: ts(p.updated_at),
                default_branch: br.map(|b| b.name),
            });
        }

        to_result(&serde_json::json!({
            "total": total,
            "page": page,
            "page_size": page_size,
            "projects": projects,
        }))
    }

    /// List the branches of a project.
    #[tool(
        name = "list_branches",
        description = "List the branches of a project. Use a branch name in the other tools to read a non-default version of the docs."
    )]
    async fn list_branches(
        &self,
        Parameters(args): Parameters<ProjectArgs>,
        Extension(resolved): Extension<ResolvedCaller>,
    ) -> Result<CallToolResult, McpError> {
        let caller = resolved.caller;
        let sc = scope(&self.state, &args.project, args.branch.as_deref(), caller).await?;
        let rows = branch::Entity::find()
            .filter(branch::Column::ProjectId.eq(sc.proj.id))
            .order_by_desc(branch::Column::IsDefault)
            .order_by_asc(branch::Column::Name)
            .all(&self.state.db)
            .await
            .map_err(crate::error::AppError::from)
            .map_err(err)?;

        let mut branches = Vec::with_capacity(rows.len());
        for b in rows.iter().take(50) {
            let count = count_docs(
                &self.state,
                sc.proj.id,
                b.head_commit_id,
                !sc.with_unpublished,
            )
            .await?;
            branches.push(BranchSummary {
                name: b.name.clone(),
                is_default: b.is_default,
                head_commit_id: b.head_commit_id.map(|v| v.to_string()),
                created_at: ts(b.created_at),
                doc_count: count,
            });
        }

        to_result(&BranchList {
            project: sc.proj.slug,
            branches,
        })
    }

    /// List the documents of a project (the knowledge base index).
    #[tool(
        name = "list_documents",
        description = "List the documents of a project. Returns path, title, size and a reading URL for each document so you can pick what to read next."
    )]
    async fn list_documents(
        &self,
        Parameters(args): Parameters<ProjectArgs>,
        Extension(resolved): Extension<ResolvedCaller>,
    ) -> Result<CallToolResult, McpError> {
        let caller = resolved.caller;
        let sc = scope(&self.state, &args.project, args.branch.as_deref(), caller).await?;
        let snap = load_snapshot(&self.state, &sc).await?;
        let visible = visible_ids(&self.state, &sc, &snap).await?;
        let internal = sc.with_unpublished;

        let documents: Vec<DocSummary> = snap
            .docs
            .iter()
            .filter(|d| visible.contains(&d.id))
            .map(|d| DocSummary {
                id: d.id.to_string(),
                path: d.path.clone(),
                title: d.title.clone(),
                size_bytes: d.size_bytes,
                updated_at: ts(d.updated_at),
                url: doc_url(&self.state, &sc.proj.slug, &d.path, internal),
            })
            .collect();

        to_result(&DocList {
            project: sc.proj.slug,
            branch: sc.br.name,
            count: documents.len(),
            documents,
        })
    }

    /// Return the heading tree of one document.
    #[tool(
        name = "get_document_outline",
        description = "Return the heading outline (level, title, anchor, line) of one document. Use it before read_document(section=...) to read only the relevant part."
    )]
    async fn get_document_outline(
        &self,
        Parameters(args): Parameters<ReadDocArgs>,
        Extension(resolved): Extension<ResolvedCaller>,
    ) -> Result<CallToolResult, McpError> {
        let caller = resolved.caller;
        if args.path.is_none() && args.doc_id.is_none() {
            return Err(McpError::invalid_params(
                "provide either `path` or `doc_id`",
                None,
            ));
        }
        let sc = scope(&self.state, &args.project, args.branch.as_deref(), caller).await?;
        let snap = load_snapshot(&self.state, &sc).await?;
        let visible = visible_ids(&self.state, &sc, &snap).await?;
        let id = args
            .doc_id
            .as_deref()
            .map(|s| Uuid::parse_str(s.trim()))
            .transpose()
            .map_err(|_| McpError::invalid_params("doc_id must be a UUID", None))?;
        let doc = snap
            .find_doc(args.path.as_deref(), id)
            .filter(|d| visible.contains(&d.id))
            .ok_or_else(|| {
                McpError::resource_not_found(
                    format!(
                        "no document matches in project '{}' (use list_documents to see valid paths)",
                        args.project
                    ),
                    None,
                )
            })?;

        let headings = heading::outline(&doc.content)
            .into_iter()
            .map(|h| OutlineHeading {
                level: h.level,
                title: h.title,
                slug: h.slug,
                line: h.line,
            })
            .collect();

        to_result(&DocOutline {
            id: doc.id.to_string(),
            path: doc.path.clone(),
            title: doc.title.clone(),
            branch: sc.br.name.clone(),
            url: doc_url(&self.state, &sc.proj.slug, &doc.path, sc.with_unpublished),
            total_lines: heading::line_count(&doc.content),
            total_bytes: doc.content.len(),
            headings,
        })
    }

    /// Read one document, in full or by section / line range.
    #[tool(
        name = "read_document",
        description = "Read a document by path or id. Pass `section` (a heading slug from get_document_outline) to read just that section, or `offset`/`limit` for a line range. The response reports total_lines/total_bytes/truncated so you can page through large documents."
    )]
    async fn read_document(
        &self,
        Parameters(args): Parameters<ReadDocArgs>,
        Extension(resolved): Extension<ResolvedCaller>,
    ) -> Result<CallToolResult, McpError> {
        let caller = resolved.caller;
        if args.path.is_none() && args.doc_id.is_none() {
            return Err(McpError::invalid_params(
                "provide either `path` or `doc_id`",
                None,
            ));
        }
        let sc = scope(&self.state, &args.project, args.branch.as_deref(), caller).await?;
        let snap = load_snapshot(&self.state, &sc).await?;
        let visible = visible_ids(&self.state, &sc, &snap).await?;
        let id = args
            .doc_id
            .as_deref()
            .map(|s| Uuid::parse_str(s.trim()))
            .transpose()
            .map_err(|_| McpError::invalid_params("doc_id must be a UUID", None))?;
        let doc = snap
            .find_doc(args.path.as_deref(), id)
            .filter(|d| visible.contains(&d.id))
            .ok_or_else(|| {
                McpError::resource_not_found(
                    format!(
                        "no document matches in project '{}' (use list_documents to see valid paths)",
                        args.project
                    ),
                    None,
                )
            })?;

        let total_lines = heading::line_count(&doc.content).max(1);
        let total_bytes = doc.content.len();
        let (mut body, line_start, line_end, section_title, truncated) =
            if let Some(sel) = args.section.as_deref().filter(|s| !s.trim().is_empty()) {
                match heading::section_range(&doc.content, sel) {
                    Some((s, e, h)) => (
                        heading::slice_lines(&doc.content, s, e),
                        s,
                        e,
                        Some(h.title),
                        false,
                    ),
                    None => {
                        let available: Vec<String> = heading::outline(&doc.content)
                            .into_iter()
                            .take(12)
                            .map(|h| h.slug)
                            .collect();
                        return Err(McpError::invalid_params(
                            format!(
                                "section '{sel}' not found; available anchors: {}",
                                available.join(", ")
                            ),
                            None,
                        ));
                    }
                }
            } else if args.offset.is_some() || args.limit.is_some() {
                let start = args.offset.unwrap_or(1).max(1);
                let limit = args.limit.unwrap_or(200).clamp(1, 5000);
                let end = start.saturating_add(limit - 1).min(total_lines);
                (
                    heading::slice_lines(&doc.content, start, end),
                    start,
                    end,
                    None,
                    end < total_lines || start > 1,
                )
            } else {
                let (text, cut) =
                    heading::truncate_bytes(&doc.content, self.state.cfg.mcp_max_content_bytes);
                (text, 1, total_lines, None, cut)
            };

        // 显式指定的行区间也受字节上限保护，避免一个巨大的 section 撑爆上下文。
        let (capped, cap_cut) =
            heading::truncate_bytes(&body, self.state.cfg.mcp_max_content_bytes);
        body = capped;

        to_result(&DocContent {
            id: doc.id.to_string(),
            path: doc.path.clone(),
            title: doc.title.clone(),
            branch: sc.br.name.clone(),
            url: doc_url(&self.state, &sc.proj.slug, &doc.path, sc.with_unpublished),
            commit_id: sc.br.head_commit_id.map(|v| v.to_string()),
            updated_at: ts(doc.updated_at),
            content: body,
            line_start,
            line_end,
            total_lines,
            total_bytes,
            truncated: truncated || cap_cut,
            section: section_title,
        })
    }

    /// Full-text search across one or more projects.
    #[tool(
        name = "search_documents",
        description = "Search one project (`project`) or several at once (`projects`) by keyword. Hits are ranked by relevance and include a reading URL, heading anchor, commit id and score; use `offset` to page."
    )]
    async fn search_documents(
        &self,
        Parameters(args): Parameters<SearchArgs>,
        Extension(resolved): Extension<ResolvedCaller>,
    ) -> Result<CallToolResult, McpError> {
        let caller = resolved.caller;
        let q = args.query.trim();
        if q.is_empty() {
            return Err(McpError::invalid_params("query must not be empty", None));
        }
        let targets: Vec<String> = match (&args.projects, &args.project) {
            (Some(list), _) if !list.is_empty() => list.clone(),
            (_, Some(p)) if !p.trim().is_empty() => vec![p.clone()],
            _ => {
                return Err(McpError::invalid_params(
                    "provide `project` or `projects`",
                    None,
                ))
            }
        };
        let limit = args.limit.unwrap_or(10).clamp(1, 50);
        let offset = args.offset.unwrap_or(0).min(2000);

        let mut hits: Vec<SearchHit> = Vec::new();
        let mut seen_projects = Vec::new();
        for slug in targets.iter().take(20) {
            let sc = match scope(&self.state, slug, args.branch.as_deref(), caller).await {
                Ok(s) => s,
                // 跨项目检索时，无权限的项目静默跳过；单项目时照常报错。
                Err(_) if targets.len() > 1 => continue,
                Err(e) => return Err(e),
            };
            seen_projects.push(sc.proj.slug.clone());
            let snap = load_snapshot(&self.state, &sc).await?;
            let visible = visible_ids(&self.state, &sc, &snap).await?;
            let internal = sc.with_unpublished;
            for d in snap.docs.iter().filter(|d| visible.contains(&d.id)) {
                let stats = search::count_matches(&d.content, q);
                if stats.occurrences == 0
                    && !d.title.to_lowercase().contains(&q.to_lowercase())
                    && !d.path.to_lowercase().contains(&q.to_lowercase())
                {
                    continue;
                }
                let score = search::relevance_with_stats(&d.title, &d.path, &d.content, q, &stats);
                let sn = stats
                    .first_pos
                    .map(|pos| snippet_at(&d.content, q, pos))
                    .unwrap_or_default();
                let anchor = stats
                    .first_pos
                    .and_then(|pos| heading::heading_at(&d.content, pos))
                    .map(|h| h.slug);
                hits.push(SearchHit {
                    project: sc.proj.slug.clone(),
                    id: d.id.to_string(),
                    path: d.path.clone(),
                    title: d.title.clone(),
                    url: doc_url(&self.state, &sc.proj.slug, &d.path, internal),
                    anchor,
                    commit_id: sc.br.head_commit_id.map(|v| v.to_string()),
                    updated_at: ts(d.updated_at),
                    score,
                    snippet: if sn.is_empty() {
                        d.content
                            .chars()
                            .take(200)
                            .collect::<String>()
                            .replace('\n', " ")
                    } else {
                        sn
                    },
                    occurrences: stats.occurrences,
                });
            }
        }

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.path.cmp(&b.path))
        });
        let total = hits.len();
        let page: Vec<SearchHit> = hits.into_iter().skip(offset).take(limit).collect();

        to_result(&SearchResults {
            projects: seen_projects,
            query: q.to_string(),
            count: total,
            offset,
            returned: page.len(),
            truncated: offset + page.len() < total,
            hits: page,
        })
    }

    /// Link graph between the documents (the knowledge graph).
    #[tool(
        name = "get_link_graph",
        description = "Return the directed link graph between the documents of a project, useful for navigating related documents and spotting dangling links."
    )]
    async fn get_link_graph(
        &self,
        Parameters(args): Parameters<ProjectArgs>,
        Extension(resolved): Extension<ResolvedCaller>,
    ) -> Result<CallToolResult, McpError> {
        let caller = resolved.caller;
        let sc = scope(&self.state, &args.project, args.branch.as_deref(), caller).await?;
        let snap = load_snapshot(&self.state, &sc).await?;
        let visible = visible_ids(&self.state, &sc, &snap).await?;
        let docs: Vec<&snapshot::DocSnapshot> = snap
            .docs
            .iter()
            .filter(|d| visible.contains(&d.id))
            .collect();
        let idx: HashMap<Uuid, usize> = docs.iter().enumerate().map(|(i, d)| (d.id, i)).collect();

        let edges: Vec<&snapshot::EdgeSnapshot> = snap
            .edges
            .iter()
            .filter(|e| idx.contains_key(&e.source_doc_id))
            .collect();

        let mut out = vec![0usize; docs.len()];
        let mut inn = vec![0usize; docs.len()];
        let mut dangling = vec![0usize; docs.len()];
        for e in &edges {
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
            project: sc.proj.slug.clone(),
            branch: sc.br.name.clone(),
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
            edges: edges
                .into_iter()
                .map(|e| GraphEdge {
                    source: e.source_doc_id.to_string(),
                    target: e.target_doc_id.map(|t| t.to_string()),
                    raw_target: e.raw_target.clone(),
                    link_type: e.link_type.clone(),
                    anchor: e.anchor.clone(),
                })
                .collect(),
            unreachable_count,
        })
    }
}

/// `#[tool_handler]` fills in `get_info` / `list_tools` / `call_tool`.
#[tool_handler(
    name = "mdocs",
    instructions = "Read-only access to Markdown documentation. Start with `list_projects`, then `list_documents` to see the index, `search_documents` to find relevant material, `get_document_outline` + `read_document(section=...)` to read only the relevant part, and `get_link_graph` to explore how documents reference each other. Search hits and documents carry `url` + `anchor`; cite them in answers. Without credentials only public projects and published documents are visible; send an API token (Settings → API token) as `Authorization: Bearer <token>` to also reach the projects you are a member of, including their unpublished documents."
)]
impl rmcp::ServerHandler for DocsServer {
    /// 统一入口：先鉴权并把结果放进请求扩展，再交给工具路由；无论成功失败都留审计。
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        mut context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let tool = request.name.to_string();
        let project = audit_project(&request.arguments);
        let detail = audit_detail(&request.arguments);

        let resolved = {
            let Some(parts) = context.extensions.get::<Parts>() else {
                return Err(McpError::internal_error("missing request context", None));
            };
            match caller(&self.state, parts).await {
                Ok((caller, info)) => ResolvedCaller { caller, info },
                Err(e) => {
                    let audit = ToolAudit::new(
                        &self.state,
                        CallerInfo {
                            user_id: None,
                            token_prefix: None,
                            client_ip: client_ip(parts),
                        },
                        tool,
                        project,
                        detail,
                    );
                    audit.finish::<CallToolResponse>(&Err(e.clone()));
                    return Err(e);
                }
            }
        };

        context.extensions.insert(resolved.clone());
        let audit = ToolAudit::new(&self.state, resolved.info.clone(), tool, project, detail);
        let out = self
            .tool_router
            .call(ToolCallContext::new(self, request, context))
            .await;
        audit.finish(&out);
        out
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let Some(parts) = parts_of(&context) else {
            return Ok(ListResourcesResult::with_all_items(vec![]));
        };
        let (caller, info) = caller(&self.state, parts).await?;
        audit_call(&self.state, &info, "list_resources", None, None);

        let mut out = Vec::new();
        for p in visible_projects(&self.state, caller)
            .await?
            .into_iter()
            .take(10)
        {
            let Some(bid) = p.default_branch_id else {
                continue;
            };
            let Some(br) = branch::Entity::find_by_id(bid)
                .one(&self.state.db)
                .await
                .map_err(crate::error::AppError::from)
                .map_err(err)?
            else {
                continue;
            };
            let can_read_all = match caller {
                Caller::Anon => false,
                Caller::User(uid) => {
                    perm::project_role(&self.state.db, &p, uid)
                        .await
                        .map_err(err)?
                        >= ProjectRole::Reader
                }
            };
            let snap = snapshot::load(&self.state, p.id, br.id, br.head_commit_id)
                .await
                .map_err(err)?;
            let published = if can_read_all {
                None
            } else {
                Some(
                    snapshot::published_ids(&self.state, p.id)
                        .await
                        .map_err(err)?,
                )
            };
            for d in snap
                .docs
                .iter()
                .filter(|d| published.as_ref().is_none_or(|ids| ids.contains(&d.id)))
                .take(100)
            {
                out.push(
                    Resource::new(
                        format!("mdocs://{}/{}", p.slug, encode_path(&d.path)),
                        d.path.clone(),
                    )
                    .with_title(d.title.clone())
                    .with_description(format!("{} · {}", p.slug, d.path))
                    .with_mime_type("text/markdown")
                    .with_size(d.size_bytes.max(0) as u64),
                );
            }
            if out.len() >= 500 {
                break;
            }
        }
        Ok(ListResourcesResult::with_all_items(out))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult::with_all_items(
            vec![ResourceTemplate::new(
                "mdocs://{project}/{path}",
                "document",
            )
            .with_title("Markdown document")
            .with_description(
                "Read a document by project slug and path, e.g. mdocs://handbook/guide/install.md",
            )
            .with_mime_type("text/markdown")],
        ))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let Some(parts) = parts_of(&context) else {
            return Err(McpError::invalid_request("missing request context", None));
        };
        let (caller, info) = caller(&self.state, parts).await?;
        audit_call(
            &self.state,
            &info,
            "read_resource",
            None,
            Some(request.uri.clone()),
        );

        let rest = request
            .uri
            .strip_prefix("mdocs://")
            .ok_or_else(|| McpError::invalid_params("uri must start with mdocs://", None))?;
        let (slug, raw_path) = rest.split_once('/').ok_or_else(|| {
            McpError::invalid_params("uri must be mdocs://{project}/{path}", None)
        })?;
        let path = decode_path(raw_path);

        let sc = scope(&self.state, slug, None, caller).await?;
        let snap = load_snapshot(&self.state, &sc).await?;
        let visible = visible_ids(&self.state, &sc, &snap).await?;
        let doc = snap
            .find_doc(Some(&path), None)
            .filter(|d| visible.contains(&d.id))
            .ok_or_else(|| {
                McpError::resource_not_found(
                    format!("no document '{}' in project '{}'", path, slug),
                    None,
                )
            })?;

        Ok(ReadResourceResponse::Complete(ReadResourceResult::new(
            vec![ResourceContents::text(doc.content.clone(), request.uri)
                .with_mime_type("text/markdown")],
        )))
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult::with_all_items(vec![Prompt::new(
            "answer_with_citations",
            Some("Answer a question from an MDocs project, citing documents and anchors."),
            Some(vec![
                PromptArgument::new("project")
                    .with_description("Project slug (see list_projects)")
                    .with_required(true),
                PromptArgument::new("question")
                    .with_description("The question to answer")
                    .with_required(true),
            ]),
        )]))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        if request.name != "answer_with_citations" {
            return Err(McpError::invalid_params(
                format!("unknown prompt '{}'", request.name),
                None,
            ));
        }
        let args = request.arguments.unwrap_or_default();
        let project = args
            .get("project")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let question = args
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if project.is_empty() || question.is_empty() {
            return Err(McpError::invalid_params(
                "prompt requires `project` and `question`",
                None,
            ));
        }

        let text = format!(
            "You are answering questions from the MDocs knowledge base.\n\
             Project: {project}\n\
             Question: {question}\n\n\
             Follow these steps:\n\
             1. Call `search_documents` with project=\"{project}\" and a short keyword query.\n\
             2. For each promising hit, call `get_document_outline`, then `read_document(section=...)` \
                (or `offset`/`limit`) to read only the relevant part. Do not dump whole long documents.\n\
             3. Answer only from the documents you actually read. If the knowledge base does not \
                contain the answer, say so explicitly instead of guessing.\n\
             4. Cite every claim with the document title, its `path` and its `url` (append `#anchor` \
                when the hit or section has an anchor).\n\
             5. Quote short snippets when the wording matters; keep the final answer concise."
        );

        Ok(GetPromptResponse::Complete(
            rmcp::model::GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)]),
        ))
    }
}

/// 当前调用者可见的全部项目（公开 + 成员）。
async fn visible_projects(
    state: &AppState,
    caller: Caller,
) -> Result<Vec<project::Model>, McpError> {
    let mut visible = sea_orm::Condition::any().add(project::Column::Visibility.eq("public"));
    if let Caller::User(uid) = caller {
        let member_of: Vec<Uuid> = project_member::Entity::find()
            .filter(project_member::Column::UserId.eq(uid))
            .all(&state.db)
            .await
            .map_err(err)?
            .into_iter()
            .map(|m| m.project_id)
            .collect();
        visible = visible.add(project::Column::OwnerId.eq(uid));
        if !member_of.is_empty() {
            visible = visible.add(project::Column::Id.is_in(member_of));
        }
    }
    Ok(project::Entity::find()
        .filter(visible)
        .order_by_desc(project::Column::UpdatedAt)
        .all(&state.db)
        .await
        .map_err(crate::error::AppError::from)
        .map_err(err)?)
}

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
