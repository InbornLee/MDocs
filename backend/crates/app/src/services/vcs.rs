//! Version-control core: commit graph, branch state resolution, atomic commits.
//!
//! Model: every write on a branch produces one commit (single parent, linear
//! chain per branch). A merge squashes applied changes into the target
//! branch's chain. Document content lives in `doc_versions`; the state of a
//! document on a branch is the nearest commit_doc (head -> root) that
//! mentions it. Branch head updates use compare-and-set for optimistic
//! concurrency.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, Set,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::entities::{branch, commit, commit_doc, doc_version, project};
use crate::error::{AppError, AppResult};

pub fn sha256_hex(content: &str) -> String {
    let mut h = Sha256::new();
    h.update(content.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Extract document title: `title:` of a leading YAML front matter block, else
/// the first `# heading` within the leading blank/comment region, else the file
/// name without extension.
pub fn extract_title(content: &str, path: &str) -> String {
    if let Some(title) = front_matter_title(content) {
        return title;
    }
    for line in content.lines().take(50) {
        if let Some(h) = line.trim().strip_prefix("# ") {
            let h = h.trim();
            if !h.is_empty() {
                return h.chars().take(200).collect();
            }
        }
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    name.trim_end_matches(".md")
        .trim_end_matches(".markdown")
        .to_string()
}

/// `title:` inside a leading `---` … `---` block. Only a block that opens on the
/// first line, closes within [`FRONT_MATTER_MAX_LINES`] and actually carries a
/// `title:` key counts, so a document that merely starts with a thematic break
/// keeps its heading-based title.
const FRONT_MATTER_MAX_LINES: usize = 50;

fn front_matter_title(content: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines.take(FRONT_MATTER_MAX_LINES) {
        let trimmed = line.trim();
        if trimmed == "---" {
            return None;
        }
        if let Some(value) = trimmed.strip_prefix("title:") {
            let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
            if !value.is_empty() {
                return Some(value.chars().take(200).collect());
            }
        }
    }
    None
}

/// Validate a document path: relative, forward slashes, no traversal.
pub fn validate_doc_path(path: &str) -> AppResult<()> {
    let bad = |r: &str| Err(AppError::BadRequest(format!("invalid path: {r}")));
    if path.is_empty() || path.len() > 512 {
        return bad("length must be 1-512");
    }
    if path.starts_with('/') || path.ends_with('/') || path.contains('\\') {
        return bad("must be relative with forward slashes");
    }
    if path.contains("..") || path.contains("//") || path.contains('\0') || path.contains(':') {
        return bad("traversal / drive letters not allowed");
    }
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." {
            return bad("empty segment");
        }
    }
    let lower = path.to_lowercase();
    if !(lower.ends_with(".md") || lower.ends_with(".markdown")) {
        return bad("only .md / .markdown files are supported");
    }
    Ok(())
}

/// In-memory commit graph of one project.
pub struct CommitGraph {
    parent: HashMap<Uuid, Option<Uuid>>,
    created_at: HashMap<Uuid, DateTime<Utc>>,
}

impl CommitGraph {
    pub async fn load(db: &impl ConnectionTrait, project_id: Uuid) -> Result<Self, DbErr> {
        let commits = commit::Entity::find()
            .filter(commit::Column::ProjectId.eq(project_id))
            .all(db)
            .await?;
        let mut parent = HashMap::new();
        let mut created_at = HashMap::new();
        for c in commits {
            parent.insert(c.id, c.parent_id);
            created_at.insert(c.id, c.created_at);
        }
        Ok(Self { parent, created_at })
    }

    /// Chain from head (index 0) to root.
    pub fn chain(&self, head: Uuid) -> Vec<Uuid> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut cur = Some(head);
        while let Some(id) = cur {
            if !seen.insert(id) {
                break; // cycle guard
            }
            out.push(id);
            cur = self.parent.get(&id).cloned().flatten();
        }
        out
    }

    /// First common ancestor of two commits.
    pub fn merge_base(&self, a: Uuid, b: Uuid) -> Option<Uuid> {
        let cb: HashSet<Uuid> = self.chain(b).into_iter().collect();
        self.chain(a).into_iter().find(|id| cb.contains(id))
    }
}

/// State of one document on a branch.
#[derive(Clone, Debug)]
pub struct DocState {
    pub document_id: Uuid,
    pub version_id: Option<Uuid>,
    pub commit_id: Uuid,
    /// added | modified | deleted | renamed
    pub change_type: String,
    pub updated_at: DateTime<Utc>,
}

fn pick_nearest(
    best: &mut Option<(DocState, usize)>,
    cd: commit_doc::Model,
    depth_of: &HashMap<Uuid, usize>,
    created_at: &HashMap<Uuid, DateTime<Utc>>,
) {
    let Some(d) = depth_of.get(&cd.commit_id) else { return };
    if best.as_ref().is_some_and(|(_, bd)| *bd <= *d) {
        return;
    }
    best.replace((
        DocState {
            document_id: cd.document_id,
            version_id: cd.version_id,
            commit_id: cd.commit_id,
            change_type: cd.change_type,
            updated_at: created_at.get(&cd.commit_id).cloned().unwrap_or_default(),
        },
        *d,
    ));
}

/// Resolve the state of one document on the branch ending at `head`.
pub async fn doc_state(
    db: &impl ConnectionTrait,
    graph: &CommitGraph,
    head: Option<Uuid>,
    document_id: Uuid,
) -> Result<Option<DocState>, DbErr> {
    let Some(head) = head else { return Ok(None) };
    let chain = graph.chain(head);
    if chain.is_empty() {
        return Ok(None);
    }
    let depth_of: HashMap<Uuid, usize> = chain.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let cds = commit_doc::Entity::find()
        .filter(commit_doc::Column::DocumentId.eq(document_id))
        .filter(commit_doc::Column::CommitId.is_in(chain))
        .all(db)
        .await?;
    let mut best: Option<(DocState, usize)> = None;
    for cd in cds {
        pick_nearest(&mut best, cd, &depth_of, &graph.created_at);
    }
    Ok(best.map(|(s, _)| s))
}

/// Resolve the current state of every document on the branch ending at `head`.
pub async fn branch_doc_states(
    db: &impl ConnectionTrait,
    graph: &CommitGraph,
    head: Option<Uuid>,
) -> Result<HashMap<Uuid, DocState>, DbErr> {
    let mut out = HashMap::new();
    let Some(head) = head else { return Ok(out) };
    let chain = graph.chain(head);
    if chain.is_empty() {
        return Ok(out);
    }
    let depth_of: HashMap<Uuid, usize> = chain.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let cds = commit_doc::Entity::find()
        .filter(commit_doc::Column::CommitId.is_in(chain))
        .all(db)
        .await?;
    let mut best: HashMap<Uuid, (DocState, usize)> = HashMap::new();
    for cd in cds {
        let Some(d) = depth_of.get(&cd.commit_id).copied() else { continue };
        if best.get(&cd.document_id).is_none_or(|(_, bd)| *bd > d) {
            let st = DocState {
                document_id: cd.document_id,
                version_id: cd.version_id,
                commit_id: cd.commit_id,
                change_type: cd.change_type.clone(),
                updated_at: graph.created_at.get(&cd.commit_id).cloned().unwrap_or_default(),
            };
            best.insert(cd.document_id, (st, d));
        }
    }
    for (id, (st, _)) in best {
        out.insert(id, st);
    }
    Ok(out)
}

/// Resolve a branch by name (or the project default).
pub async fn resolve_branch(
    db: &impl ConnectionTrait,
    proj: &project::Model,
    name: Option<&str>,
) -> AppResult<branch::Model> {
    let mut q = branch::Entity::find().filter(branch::Column::ProjectId.eq(proj.id));
    q = match name {
        Some(n) => q.filter(branch::Column::Name.eq(n)),
        None => q.filter(branch::Column::IsDefault.eq(true)),
    };
    q.one(db)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::NotFound("branch not found".into()))
}

/// One document change inside a commit.
pub struct Change {
    pub document_id: Uuid,
    /// added | modified | deleted | renamed
    pub change_type: &'static str,
    pub version_id: Option<Uuid>,
    pub old_path: Option<String>,
}

/// Insert a commit with its changes and advance the branch head via CAS.
/// Must run inside the caller's transaction (`db` may be `&DatabaseTransaction`).
pub async fn create_commit<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    branch_id: Uuid,
    expected_head: Option<Uuid>,
    author_id: Uuid,
    message: &str,
    commit_type: &str,
    changes: Vec<Change>,
) -> AppResult<commit::Model> {
    let now = Utc::now();
    let cm = commit::ActiveModel {
        id: Set(Uuid::now_v7()),
        project_id: Set(project_id),
        branch_id: Set(branch_id),
        parent_id: Set(expected_head),
        author_id: Set(author_id),
        message: Set(message.to_string()),
        commit_type: Set(commit_type.to_string()),
        created_at: Set(now),
    };
    let created = cm.insert(db).await?;

    for ch in changes {
        commit_doc::ActiveModel {
            id: Set(Uuid::now_v7()),
            commit_id: Set(created.id),
            document_id: Set(ch.document_id),
            change_type: Set(ch.change_type.to_string()),
            version_id: Set(ch.version_id),
            old_path: Set(ch.old_path),
        }
        .insert(db)
        .await?;
    }

    // CAS the branch head; 0 rows => someone else advanced it first.
    let mut q = branch::Entity::update_many()
        .col_expr(
            branch::Column::HeadCommitId,
            sea_orm::sea_query::Expr::value(created.id),
        )
        .filter(branch::Column::Id.eq(branch_id));
    q = match expected_head {
        Some(h) => q.filter(branch::Column::HeadCommitId.eq(h)),
        None => q.filter(branch::Column::HeadCommitId.is_null()),
    };
    let res = q.exec(db).await?;
    if res.rows_affected == 0 {
        return Err(AppError::Conflict(
            "branch advanced concurrently, retry".into(),
        ));
    }
    Ok(created)
}

/// Store a new immutable version of a document. Caller decides inside which
/// transaction.
pub async fn create_version(
    db: &impl ConnectionTrait,
    document_id: Uuid,
    content: &str,
    created_by: Uuid,
) -> Result<doc_version::Model, DbErr> {
    let m = doc_version::ActiveModel {
        id: Set(Uuid::now_v7()),
        document_id: Set(document_id),
        content: Set(content.to_string()),
        content_hash: Set(sha256_hex(content)),
        size_bytes: Set(content.len() as i32),
        created_by: Set(created_by),
        created_at: Set(Utc::now()),
    };
    m.insert(db).await
}
