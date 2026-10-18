//! Merge requests: create, live preview, reviews, three-way merge, close.
//!
//! Semantics: `ours` is the target branch, `theirs` the source branch, and
//! `base` the merge-base commit (nearest common ancestor). Merging squashes
//! the resolved plan into a single `merge` commit on the target branch
//! (consistent with the linear-chain-per-branch model in `vcs`).

use std::collections::{HashMap, HashSet};

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{branch, doc_version, document, merge_request, mr_review, project, user};
use crate::error::{AppError, AppResult};
use crate::services::{
    auth::CurrentUser, links, links::LinkUpdate, merge, perm, perm::Role, vcs, vcs::DocState,
};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects/{slug}/mrs", get(list_mrs).post(create_mr))
        .route("/api/projects/{slug}/mrs/{mr_id}", get(get_mr_detail))
        .route("/api/projects/{slug}/mrs/{mr_id}/reviews", post(add_review))
        .route("/api/projects/{slug}/mrs/{mr_id}/merge", post(merge_mr))
        .route("/api/projects/{slug}/mrs/{mr_id}/close", post(close_mr))
}

// ---------- shared helpers ----------

async fn load_mr(
    db: &sea_orm::DatabaseConnection,
    proj: &project::Model,
    mr_id: Uuid,
) -> AppResult<merge_request::Model> {
    merge_request::Entity::find_by_id(mr_id)
        .one(db)
        .await?
        .filter(|m| m.project_id == proj.id)
        .ok_or_else(|| AppError::NotFound("merge request not found".into()))
}

async fn load_branch(
    db: &sea_orm::DatabaseConnection,
    proj: &project::Model,
    id: Uuid,
) -> AppResult<branch::Model> {
    branch::Entity::find_by_id(id)
        .one(db)
        .await?
        .filter(|b| b.project_id == proj.id)
        .ok_or_else(|| AppError::NotFound("branch not found".into()))
}

/// Compare a doc's state across branches: identical (version, change_type)
/// means "no difference on that side".
fn state_key(st: Option<&DocState>) -> (Option<Uuid>, String) {
    match st {
        None => (None, "absent".into()),
        Some(s) => (s.version_id, s.change_type.clone()),
    }
}

fn is_alive(st: Option<&DocState>) -> bool {
    st.is_some_and(|s| s.change_type != "deleted")
}

async fn load_contents(
    db: &sea_orm::DatabaseConnection,
    ids: Vec<Uuid>,
) -> AppResult<HashMap<Uuid, String>> {
    let mut out = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let vers = doc_version::Entity::find()
        .filter(doc_version::Column::Id.is_in(ids))
        .all(db)
        .await?;
    for v in vers {
        out.insert(v.id, v.content);
    }
    Ok(out)
}

/// One file in the merge plan: what three-way merge produces for it.
#[derive(Serialize, Clone)]
struct FilePlan {
    document_id: Uuid,
    path: String,
    title: String,
    /// added | modified | deleted (planned change on the target branch)
    change: String,
    has_conflict: bool,
    /// content | modify-delete | add-add
    conflict_kind: Option<String>,
    base_content: Option<String>,
    /// target-branch side
    ours_content: Option<String>,
    /// source-branch side
    theirs_content: Option<String>,
    /// merged result; None when the file ends up deleted
    merged_content: Option<String>,
}

/// Live three-way merge plan between source and target heads.
/// Returns (merge_base_commit, plan).
async fn compute_plan(
    db: &sea_orm::DatabaseConnection,
    proj: &project::Model,
    source: &branch::Model,
    target: &branch::Model,
) -> AppResult<(Option<Uuid>, Vec<FilePlan>)> {
    let graph = vcs::CommitGraph::load(db, proj.id).await?;

    let base_commit = match (source.head_commit_id, target.head_commit_id) {
        (Some(s), Some(t)) => graph.merge_base(s, t),
        _ => None,
    };

    let states_base = match base_commit {
        Some(b) => vcs::branch_doc_states(db, &graph, Some(b)).await?,
        None => HashMap::new(),
    };
    let states_ours = vcs::branch_doc_states(db, &graph, target.head_commit_id).await?;
    let states_theirs = vcs::branch_doc_states(db, &graph, source.head_commit_id).await?;

    // doc universe: anything the two branch tips differ on
    let mut ids: HashSet<Uuid> = states_base.keys().copied().collect();
    ids.extend(states_ours.keys().copied());
    ids.extend(states_theirs.keys().copied());

    // collect needed version contents in one query
    let mut ver_ids: Vec<Uuid> = Vec::new();
    for st in states_base
        .values()
        .chain(states_ours.values())
        .chain(states_theirs.values())
    {
        if let Some(v) = st.version_id {
            ver_ids.push(v);
        }
    }
    let contents = load_contents(db, ver_ids).await?;

    let docs = document::Entity::find()
        .filter(document::Column::ProjectId.eq(proj.id))
        .filter(document::Column::Id.is_in(ids.iter().copied()))
        .all(db)
        .await?;
    let doc_by_id: HashMap<Uuid, document::Model> = docs.into_iter().map(|d| (d.id, d)).collect();

    let mut plan: Vec<FilePlan> = Vec::new();
    for id in ids {
        let Some(doc) = doc_by_id.get(&id) else {
            continue;
        };
        let b = states_base.get(&id);
        let o = states_ours.get(&id);
        let t = states_theirs.get(&id);

        // source did not change this doc, or the target already holds the
        // exact same state (e.g. change merged previously)
        if state_key(t) == state_key(b) || state_key(o) == state_key(t) {
            continue;
        }
        // target untouched: take theirs verbatim
        if state_key(o) == state_key(b) {
            if is_alive(t) {
                let content = t
                    .and_then(|s| s.version_id)
                    .and_then(|v| contents.get(&v).cloned());
                plan.push(FilePlan {
                    document_id: id,
                    path: doc.path.clone(),
                    title: doc.title.clone(),
                    change: if is_alive(b) {
                        "modified".into()
                    } else {
                        "added".into()
                    },
                    has_conflict: false,
                    conflict_kind: None,
                    base_content: b
                        .and_then(|s| s.version_id)
                        .and_then(|v| contents.get(&v).cloned()),
                    ours_content: o
                        .and_then(|s| s.version_id)
                        .and_then(|v| contents.get(&v).cloned()),
                    theirs_content: content.clone(),
                    merged_content: content,
                });
            } else {
                plan.push(FilePlan {
                    document_id: id,
                    path: doc.path.clone(),
                    title: doc.title.clone(),
                    change: "deleted".into(),
                    has_conflict: false,
                    conflict_kind: None,
                    base_content: b
                        .and_then(|s| s.version_id)
                        .and_then(|v| contents.get(&v).cloned()),
                    ours_content: o
                        .and_then(|s| s.version_id)
                        .and_then(|v| contents.get(&v).cloned()),
                    theirs_content: None,
                    merged_content: None,
                });
            }
            continue;
        }

        // both sides changed this doc
        match (is_alive(t), is_alive(o)) {
            (false, false) => {
                // both deleted it — same outcome, no conflict
                plan.push(FilePlan {
                    document_id: id,
                    path: doc.path.clone(),
                    title: doc.title.clone(),
                    change: "deleted".into(),
                    has_conflict: false,
                    conflict_kind: None,
                    base_content: b
                        .and_then(|s| s.version_id)
                        .and_then(|v| contents.get(&v).cloned()),
                    ours_content: None,
                    theirs_content: None,
                    merged_content: None,
                });
            }
            (false, true) | (true, false) => {
                // modify/delete conflict — needs manual resolution
                plan.push(FilePlan {
                    document_id: id,
                    path: doc.path.clone(),
                    title: doc.title.clone(),
                    change: if is_alive(t) {
                        "modified".into()
                    } else {
                        "deleted".into()
                    },
                    has_conflict: true,
                    conflict_kind: Some("modify-delete".into()),
                    base_content: b
                        .and_then(|s| s.version_id)
                        .and_then(|v| contents.get(&v).cloned()),
                    ours_content: o
                        .and_then(|s| s.version_id)
                        .and_then(|v| contents.get(&v).cloned()),
                    theirs_content: t
                        .and_then(|s| s.version_id)
                        .and_then(|v| contents.get(&v).cloned()),
                    merged_content: if is_alive(o) {
                        o.and_then(|s| s.version_id)
                            .and_then(|v| contents.get(&v).cloned())
                    } else {
                        t.and_then(|s| s.version_id)
                            .and_then(|v| contents.get(&v).cloned())
                    },
                });
            }
            (true, true) => {
                let base_txt = b
                    .and_then(|s| s.version_id)
                    .and_then(|v| contents.get(&v).cloned())
                    .unwrap_or_default();
                let ours_txt = o
                    .and_then(|s| s.version_id)
                    .and_then(|v| contents.get(&v).cloned())
                    .unwrap_or_default();
                let theirs_txt = t
                    .and_then(|s| s.version_id)
                    .and_then(|v| contents.get(&v).cloned())
                    .unwrap_or_default();
                let outcome = merge::merge_file(&base_txt, &ours_txt, &theirs_txt);
                plan.push(FilePlan {
                    document_id: id,
                    path: doc.path.clone(),
                    title: doc.title.clone(),
                    change: "modified".into(),
                    has_conflict: outcome.has_conflict,
                    conflict_kind: outcome.has_conflict.then(|| {
                        if is_alive(b) {
                            "content".into()
                        } else {
                            "add-add".into()
                        }
                    }),
                    base_content: b
                        .and_then(|s| s.version_id)
                        .and_then(|v| contents.get(&v).cloned()),
                    ours_content: Some(ours_txt),
                    theirs_content: Some(theirs_txt),
                    merged_content: Some(outcome.content),
                });
            }
        }
    }
    plan.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((base_commit, plan))
}

// ---------- DTOs ----------

#[derive(Serialize)]
struct MrListItem {
    id: Uuid,
    title: String,
    status: String,
    source: String,
    target: String,
    target_is_default: bool,
    conflict_files: Vec<String>,
    created_by: String,
    created_at: DateTime<Utc>,
    merged_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct ReviewDto {
    id: Uuid,
    reviewer_id: Uuid,
    reviewer: String,
    action: String,
    comment: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct MrDetailDto {
    id: Uuid,
    title: String,
    description: Option<String>,
    status: String,
    source: String,
    source_id: Uuid,
    target: String,
    target_id: Uuid,
    target_is_default: bool,
    base_commit_id: Option<Uuid>,
    merge_commit_id: Option<Uuid>,
    created_by: String,
    created_by_id: Uuid,
    created_at: DateTime<Utc>,
    merged_at: Option<DateTime<Utc>>,
    merge_base: Option<Uuid>,
    plan: Vec<FilePlan>,
    reviews: Vec<ReviewDto>,
    my_role: String,
    /// merge is executable right now (ignoring conflicts) — approval check
    can_merge: bool,
    /// conflicts currently present in the plan
    conflicts: Vec<String>,
    /// why merging is blocked (permission / approval / conflicts)
    blocked_reason: Option<String>,
}

async fn user_name(db: &sea_orm::DatabaseConnection, id: Uuid) -> AppResult<String> {
    let u = user::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| AppError::Internal("user missing".into()))?;
    Ok(if u.display_name.is_empty() {
        u.username
    } else {
        u.display_name
    })
}

// ---------- list ----------

#[derive(Deserialize)]
struct ListMrsQuery {
    status: Option<String>,
}

async fn list_mrs(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
    Query(q): Query<ListMrsQuery>,
) -> AppResult<Json<Vec<MrListItem>>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Reader).await?;

    let mut mq = merge_request::Entity::find().filter(merge_request::Column::ProjectId.eq(proj.id));
    if let Some(s) = q.status.as_deref().filter(|s| !s.is_empty() && *s != "all") {
        mq = mq.filter(merge_request::Column::Status.eq(s));
    }
    let mrs = mq.all(&state.db).await?;

    let branch_ids: HashSet<Uuid> = mrs
        .iter()
        .flat_map(|m| [m.source_branch_id, m.target_branch_id])
        .collect();
    let branches = if branch_ids.is_empty() {
        vec![]
    } else {
        branch::Entity::find()
            .filter(branch::Column::Id.is_in(branch_ids))
            .all(&state.db)
            .await?
    };
    let bname: HashMap<Uuid, branch::Model> = branches.into_iter().map(|b| (b.id, b)).collect();

    let mut out = Vec::with_capacity(mrs.len());
    for m in mrs {
        let source = bname
            .get(&m.source_branch_id)
            .map(|b| b.name.clone())
            .unwrap_or_default();
        let target_b = bname.get(&m.target_branch_id);
        let target = target_b.map(|b| b.name.clone()).unwrap_or_default();
        let conflict_files = m
            .conflict_files
            .as_deref()
            .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
            .unwrap_or_default();
        out.push(MrListItem {
            id: m.id,
            title: m.title,
            status: m.status,
            source,
            target,
            target_is_default: target_b.is_some_and(|b| b.is_default),
            conflict_files,
            created_by: user_name(&state.db, m.created_by).await?,
            created_at: m.created_at,
            merged_at: m.merged_at,
        });
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(Json(out))
}

// ---------- create ----------

#[derive(Deserialize)]
struct CreateMrReq {
    source: String,
    #[serde(default)]
    target: Option<String>,
    title: String,
    #[serde(default)]
    description: Option<String>,
}

async fn create_mr(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path(slug): Path<String>,
    Json(req): Json<CreateMrReq>,
) -> AppResult<Json<serde_json::Value>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Reader).await?;

    let title = req.title.trim().to_string();
    if title.is_empty() || title.len() > 255 {
        return Err(AppError::BadRequest("title must be 1-255 chars".into()));
    }

    let source = vcs::resolve_branch(&state.db, &proj, Some(req.source.trim())).await?;
    let target =
        vcs::resolve_branch(&state.db, &proj, req.target.as_deref().map(str::trim)).await?;
    if source.id == target.id {
        return Err(AppError::BadRequest(
            "source and target branch are the same".into(),
        ));
    }
    if source.head_commit_id.is_none() {
        return Err(AppError::BadRequest("source branch is empty".into()));
    }
    if source.head_commit_id == target.head_commit_id {
        return Err(AppError::BadRequest(
            "source branch has no changes ahead of target".into(),
        ));
    }

    // reject duplicates: an open MR for the same source->target pair
    let dup = merge_request::Entity::find()
        .filter(merge_request::Column::ProjectId.eq(proj.id))
        .filter(merge_request::Column::SourceBranchId.eq(source.id))
        .filter(merge_request::Column::TargetBranchId.eq(target.id))
        .filter(merge_request::Column::Status.eq("open"))
        .one(&state.db)
        .await?;
    if dup.is_some() {
        return Err(AppError::Conflict(
            "an open merge request already exists for this branch pair".into(),
        ));
    }

    let (base, plan) = compute_plan(&state.db, &proj, &source, &target).await?;
    if plan.is_empty() {
        return Err(AppError::BadRequest(
            "nothing to merge: source has no effective changes".into(),
        ));
    }
    let conflicts: Vec<String> = plan
        .iter()
        .filter(|p| p.has_conflict)
        .map(|p| p.path.clone())
        .collect();

    let mr = merge_request::ActiveModel {
        id: Set(Uuid::now_v7()),
        project_id: Set(proj.id),
        source_branch_id: Set(source.id),
        target_branch_id: Set(target.id),
        base_commit_id: Set(base.unwrap_or_default()),
        title: Set(title),
        description: Set(req.description.filter(|d| !d.trim().is_empty())),
        status: Set("open".into()),
        conflict_files: Set(serde_json::to_string(&conflicts).ok()),
        merge_commit_id: Set(None),
        created_by: Set(cur.id),
        reviewed_by: Set(None),
        merged_at: Set(None),
        created_at: Set(Utc::now()),
    }
    .insert(&state.db)
    .await?;

    Ok(Json(serde_json::json!({
        "id": mr.id,
        "status": mr.status,
        "conflicts": conflicts,
        "changed_files": plan.len(),
    })))
}

// ---------- detail ----------

async fn get_mr_detail(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path((slug, mr_id)): Path<(String, Uuid)>,
) -> AppResult<Json<MrDetailDto>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    let role = perm::require_role(&state.db, &proj, cur.id, Role::Reader).await?;
    let m = load_mr(&state.db, &proj, mr_id).await?;
    let source = load_branch(&state.db, &proj, m.source_branch_id).await?;
    let target = load_branch(&state.db, &proj, m.target_branch_id).await?;

    let (merge_base, plan) = if m.status == "open" {
        compute_plan(&state.db, &proj, &source, &target).await?
    } else {
        (None, vec![])
    };
    let conflicts: Vec<String> = plan
        .iter()
        .filter(|p| p.has_conflict)
        .map(|p| p.path.clone())
        .collect();

    let reviews_db = mr_review::Entity::find()
        .filter(mr_review::Column::MrId.eq(m.id))
        .all(&state.db)
        .await?;
    let mut reviews = Vec::with_capacity(reviews_db.len());
    for r in reviews_db {
        reviews.push(ReviewDto {
            id: r.id,
            reviewer_id: r.reviewer_id,
            reviewer: user_name(&state.db, r.reviewer_id).await?,
            action: r.action,
            comment: r.comment,
            created_at: r.created_at,
        });
    }
    reviews.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    let can_merge =
        role >= Role::Editor && (!target.is_default || has_approval(&state.db, &proj, &m).await?);
    let blocked_reason = if m.status != "open" {
        None
    } else if !can_merge {
        Some("需要项目编辑权限".to_string())
    } else if target.is_default && !has_approval(&state.db, &proj, &m).await? {
        Some("目标为默认分支，需要其他编辑者审批后才能合并".to_string())
    } else if !conflicts.is_empty() {
        Some(format!("存在 {} 个冲突文件，需先解决冲突", conflicts.len()))
    } else {
        None
    };

    Ok(Json(MrDetailDto {
        id: m.id,
        title: m.title,
        description: m.description,
        status: m.status,
        source: source.name,
        source_id: source.id,
        target: target.name,
        target_id: target.id,
        target_is_default: target.is_default,
        base_commit_id: (m.base_commit_id != Uuid::nil()).then_some(m.base_commit_id),
        merge_commit_id: m.merge_commit_id,
        created_by: user_name(&state.db, m.created_by).await?,
        created_by_id: m.created_by,
        created_at: m.created_at,
        merged_at: m.merged_at,
        merge_base,
        plan,
        reviews,
        my_role: role.as_str().into(),
        can_merge,
        conflicts,
        blocked_reason,
    }))
}

/// An approving review from another project member with editor+ permission.
async fn has_approval(
    db: &sea_orm::DatabaseConnection,
    proj: &project::Model,
    m: &merge_request::Model,
) -> AppResult<bool> {
    let reviews = mr_review::Entity::find()
        .filter(mr_review::Column::MrId.eq(m.id))
        .all(db)
        .await?;
    for r in reviews {
        if r.action != "approve" || r.reviewer_id == m.created_by {
            continue;
        }
        if perm::project_role(db, proj, r.reviewer_id).await? >= Role::Editor {
            return Ok(true);
        }
    }
    Ok(false)
}

// ---------- reviews ----------

#[derive(Deserialize)]
struct AddReviewReq {
    /// approve | reject | comment
    action: String,
    #[serde(default)]
    comment: Option<String>,
}

async fn add_review(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path((slug, mr_id)): Path<(String, Uuid)>,
    Json(req): Json<AddReviewReq>,
) -> AppResult<Json<serde_json::Value>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Reader).await?;
    let m = load_mr(&state.db, &proj, mr_id).await?;
    if m.status != "open" {
        return Err(AppError::BadRequest("merge request is not open".into()));
    }
    let action = match req.action.as_str() {
        "approve" | "reject" | "comment" => req.action.clone(),
        _ => {
            return Err(AppError::BadRequest(
                "action must be approve|reject|comment".into(),
            ))
        }
    };
    let comment = req
        .comment
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty() && c.len() <= 2000);

    let r = mr_review::ActiveModel {
        id: Set(Uuid::now_v7()),
        mr_id: Set(m.id),
        reviewer_id: Set(cur.id),
        action: Set(action),
        comment: Set(comment),
        created_at: Set(Utc::now()),
    }
    .insert(&state.db)
    .await?;
    Ok(Json(serde_json::json!({ "id": r.id, "action": r.action })))
}

// ---------- merge ----------

#[derive(Deserialize)]
struct MergeMrReq {
    /// docId -> resolved content (null / missing "delete" semantics handled
    /// via `deletes`), only needed for conflicted files
    #[serde(default)]
    resolutions: HashMap<String, Option<String>>,
    #[serde(default)]
    deletes: Vec<String>,
    #[serde(default)]
    message: Option<String>,
}

async fn merge_mr(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path((slug, mr_id)): Path<(String, Uuid)>,
    Json(req): Json<MergeMrReq>,
) -> AppResult<Json<serde_json::Value>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    perm::require_role(&state.db, &proj, cur.id, Role::Editor).await?;
    let m = load_mr(&state.db, &proj, mr_id).await?;
    if m.status != "open" {
        return Err(AppError::BadRequest(format!(
            "merge request is {}",
            m.status
        )));
    }
    let source = load_branch(&state.db, &proj, m.source_branch_id).await?;
    let target = load_branch(&state.db, &proj, m.target_branch_id).await?;

    // default branch = protected: needs a second pair of eyes
    if target.is_default && !has_approval(&state.db, &proj, &m).await? {
        return Err(AppError::Forbidden(
            "target is the default branch: approval from another editor is required".into(),
        ));
    }

    let (_, plan) = compute_plan(&state.db, &proj, &source, &target).await?;
    if plan.is_empty() {
        return Err(AppError::BadRequest(
            "nothing to merge: source has no changes ahead of target".into(),
        ));
    }

    let deletes: HashSet<String> = req.deletes.iter().cloned().collect();

    // resolve outcomes: (plan, final content or delete)
    struct Resolved {
        plan: FilePlan,
        content: Option<String>,
    }
    let mut resolved: Vec<Resolved> = Vec::new();
    for p in plan {
        if !p.has_conflict {
            let content = p.merged_content.clone();
            resolved.push(Resolved { plan: p, content });
            continue;
        }
        let key = p.document_id.to_string();
        let resolution = req.resolutions.get(&key);
        let content = if deletes.contains(&key) {
            None // explicitly resolved as delete
        } else if let Some(Some(txt)) = resolution {
            Some(txt.clone())
        } else if let Some(None) = resolution {
            None
        } else {
            return Err(AppError::Conflict(format!(
                "unresolved conflict in '{}': provide a resolution",
                p.path
            )));
        };
        if let Some(txt) = &content {
            if txt.contains("<<<<<<< ours") || txt.contains(">>>>>>> theirs") {
                return Err(AppError::BadRequest(format!(
                    "resolution for '{}' still contains conflict markers",
                    p.path
                )));
            }
        }
        resolved.push(Resolved { plan: p, content });
    }

    // build + apply the merge commit atomically
    let txn = state.db.begin().await?;
    let graph = vcs::CommitGraph::load(&txn, proj.id).await?;
    let mut changes: Vec<vcs::Change> = Vec::new();
    let mut pending_links: Vec<(Uuid, String, Option<String>)> = Vec::new();
    let mut title_updates: Vec<(Uuid, String)> = Vec::new();
    let now = Utc::now();

    for r in &resolved {
        let doc = document::Entity::find_by_id(r.plan.document_id)
            .one(&txn)
            .await?
            .ok_or_else(|| AppError::Internal("document missing".into()))?;
        match &r.content {
            None => {
                changes.push(vcs::Change {
                    document_id: doc.id,
                    change_type: "deleted",
                    version_id: None,
                    old_path: Some(doc.path.clone()),
                });
                pending_links.push((doc.id, doc.path.clone(), None));
            }
            Some(content) => {
                let ver = vcs::create_version(&txn, doc.id, content, cur.id).await?;
                // "added" when the doc is absent/deleted on the target branch
                let ours_alive = vcs::doc_state(&txn, &graph, target.head_commit_id, doc.id)
                    .await?
                    .is_some_and(|s| s.change_type != "deleted");
                let change_type = if ours_alive { "modified" } else { "added" };
                changes.push(vcs::Change {
                    document_id: doc.id,
                    change_type,
                    version_id: Some(ver.id),
                    old_path: None,
                });
                pending_links.push((doc.id, doc.path.clone(), Some(content.clone())));
                title_updates.push((doc.id, vcs::extract_title(content, &doc.path)));
            }
        }
    }

    let message = req
        .message
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| format!("merge '{}' into '{}'", source.name, target.name));
    let commit = vcs::create_commit(
        &txn,
        proj.id,
        target.id,
        target.head_commit_id,
        cur.id,
        &message,
        "merge",
        changes,
    )
    .await?;

    for (doc_id, title) in title_updates {
        let mut am: document::ActiveModel = document::Entity::find_by_id(doc_id)
            .one(&txn)
            .await?
            .ok_or_else(|| AppError::Internal("document missing".into()))?
            .into_active_model();
        am.title = Set(title);
        am.updated_at = Set(now);
        am.update(&txn).await?;
    }

    let link_updates: Vec<LinkUpdate> = pending_links
        .iter()
        .map(|(doc_id, path, content)| LinkUpdate {
            doc_id: *doc_id,
            path,
            content: content.as_deref(),
        })
        .collect();
    links::after_commit(&txn, proj.id, target.id, Some(commit.id), &link_updates).await?;

    let mut am: merge_request::ActiveModel = m.into_active_model();
    am.status = Set("merged".into());
    am.merge_commit_id = Set(Some(commit.id));
    am.reviewed_by = Set(Some(cur.id));
    am.merged_at = Set(Some(now));
    am.conflict_files = Set(None);
    let merged = am.update(&txn).await?;

    txn.commit().await?;

    Ok(Json(serde_json::json!({
        "id": merged.id,
        "status": merged.status,
        "merge_commit_id": commit.id,
        "files": resolved.len(),
        "conflicts_resolved": req.resolutions.len() + req.deletes.len(),
    })))
}

// ---------- close ----------

async fn close_mr(
    State(state): State<AppState>,
    Extension(cur): Extension<CurrentUser>,
    Path((slug, mr_id)): Path<(String, Uuid)>,
) -> AppResult<Json<serde_json::Value>> {
    let proj = perm::get_project(&state.db, &slug).await?;
    let role = perm::require_role(&state.db, &proj, cur.id, Role::Reader).await?;
    let m = load_mr(&state.db, &proj, mr_id).await?;
    if m.status != "open" {
        return Err(AppError::BadRequest(format!(
            "merge request is {}",
            m.status
        )));
    }
    if role < Role::Owner && m.created_by != cur.id {
        return Err(AppError::Forbidden(
            "only the author or an owner can close".into(),
        ));
    }
    let mut am: merge_request::ActiveModel = m.into_active_model();
    am.status = Set("closed".into());
    let closed = am.update(&state.db).await?;
    Ok(Json(
        serde_json::json!({ "id": closed.id, "status": closed.status }),
    ))
}
