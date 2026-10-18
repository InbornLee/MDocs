//! Link tree: extract markdown links (relative md links + wiki links) with
//! comrak, resolve them against the live documents of a branch, and maintain
//! the `link_edges` table incrementally after every commit.

use std::collections::{HashMap, HashSet};

use comrak::{parse_document, Arena, Options};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, IntoActiveModel,
    QueryFilter, Set,
};
use uuid::Uuid;

use crate::entities::{document, link_edge};
use crate::services::vcs;

/// A raw link extracted from a document (target as written, before resolution).
#[derive(Debug, Clone)]
pub struct RawLink {
    /// Target as written in the document (anchor stripped).
    pub raw_target: String,
    /// md_relative | wiki
    pub link_type: &'static str,
    pub anchor: Option<String>,
    pub display_text: Option<String>,
    pub line_no: i32,
}

fn is_external(url: &str) -> bool {
    url.is_empty()
        || url.starts_with('#')
        || url.starts_with('/')
        || url.contains("://")
        || url.starts_with("mailto:")
        || url.starts_with("tel:")
        || url.starts_with("data:")
}

/// Split `path#anchor` into (path, anchor).
fn split_anchor(url: &str) -> (String, Option<String>) {
    match url.split_once('#') {
        Some((p, a)) => (p.to_string(), Some(a.to_string())),
        None => (url.to_string(), None),
    }
}

fn first_text(node: comrak::Node<'_>) -> Option<String> {
    for n in node.descendants() {
        if let comrak::nodes::NodeValue::Text(t) = &n.data.borrow().value {
            let s = t.to_string();
            if !s.trim().is_empty() {
                return Some(s.chars().take(100).collect());
            }
        }
    }
    None
}

/// Extract all internal links from markdown content.
/// Supports `[text](relative.md#anchor)` and wiki links `[[target|text]]`.
pub fn extract_links(content: &str) -> Vec<RawLink> {
    let arena = Arena::new();
    let mut options = Options::default();
    options.extension.wikilinks_title_after_pipe = true;
    let root = parse_document(&arena, content, &options);

    let mut out = Vec::new();
    // `link_edges` keeps one row per (branch, source doc, raw target, line), so
    // the same target written twice on one line (e.g. two columns of a table
    // cell pointing at the same document) must not be emitted twice.
    let mut seen: HashSet<(String, i32)> = HashSet::new();
    for node in root.descendants() {
        let data = node.data.borrow();
        let (raw_url, link_type, display) = match &data.value {
            comrak::nodes::NodeValue::Link(l) => (l.url.clone(), "md_relative", first_text(node)),
            comrak::nodes::NodeValue::WikiLink(w) => (w.url.clone(), "wiki", first_text(node)),
            _ => continue,
        };
        drop(data);

        let url = raw_url.trim();
        if is_external(url) || url.is_empty() {
            continue;
        }
        let url = url.trim_start_matches("./");
        let (target, anchor) = split_anchor(url);
        // `..` segments are fine: clean_path rejects targets that escape the
        // project root, so only genuinely unsafe paths are dropped there.
        if target.is_empty() || target.contains('\\') {
            continue;
        }
        let line_no = node.data.borrow().sourcepos.start.line as i32;
        if !seen.insert((target.clone(), line_no)) {
            continue;
        }
        out.push(RawLink {
            raw_target: target,
            link_type,
            anchor,
            display_text: display,
            line_no,
        });
    }
    out
}

/// Resolve raw link targets against the live document set of a branch.
#[derive(Debug, Clone, Default)]
pub struct PathResolver {
    by_path: HashMap<String, Uuid>,
    by_base: HashMap<String, Vec<Uuid>>,
}

/// Normalize a `/`-joined relative path (resolve `.` and `..` segments).
pub(crate) fn clean_path(dir: &str, rel: &str) -> Option<String> {
    let rel = rel.trim().trim_start_matches("./");
    let joined: Vec<&str> = if rel.starts_with('/') || dir.is_empty() {
        // absolute target, or already at the root: use it as-is
        rel.split('/').collect()
    } else {
        let mut v: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
        v.extend(rel.split('/'));
        v
    };
    let mut stack: Vec<&str> = Vec::with_capacity(joined.len());
    for seg in joined {
        match seg {
            "" | "." => {}
            ".." => {
                // needs a segment to pop, otherwise it escapes the project root
                stack.pop()?;
            }
            s => stack.push(s),
        }
    }
    if stack.is_empty() {
        return None;
    }
    Some(stack.join("/"))
}

impl PathResolver {
    pub fn new(paths: impl Iterator<Item = (String, Uuid)>) -> Self {
        let mut r = PathResolver {
            by_path: HashMap::new(),
            by_base: HashMap::new(),
        };
        for (p, id) in paths {
            r.by_base
                .entry(p.rsplit('/').next().unwrap_or(&p).to_string())
                .or_default()
                .push(id);
            r.by_path.insert(p, id);
        }
        r
    }

    pub fn resolve(&self, source_dir: &str, raw_target: &str, link_type: &str) -> Option<Uuid> {
        match link_type {
            "wiki" => {
                // exact path, then +.md, then unique basename (Obsidian-style)
                if let Some(id) = self.by_path.get(raw_target) {
                    return Some(*id);
                }
                let with_ext = if raw_target.to_lowercase().ends_with(".md")
                    || raw_target.to_lowercase().ends_with(".markdown")
                {
                    raw_target.to_string()
                } else {
                    format!("{raw_target}.md")
                };
                if let Some(id) = self.by_path.get(&with_ext) {
                    return Some(*id);
                }
                let base = with_ext.rsplit('/').next().unwrap_or(&with_ext);
                match self.by_base.get(base).map(|v| v.as_slice()) {
                    Some([id]) => Some(*id),
                    _ => None,
                }
            }
            _ => {
                let path = clean_path(source_dir, raw_target)?;
                if let Some(id) = self.by_path.get(&path) {
                    return Some(*id);
                }
                // links commonly omit the extension
                if !path.to_lowercase().ends_with(".md")
                    && !path.to_lowercase().ends_with(".markdown")
                {
                    self.by_path.get(&format!("{path}.md")).copied()
                } else {
                    None
                }
            }
        }
    }
}

/// One document whose links need re-extraction after a commit
/// (`content: None` means the document was deleted on the branch).
pub struct LinkUpdate<'a> {
    pub doc_id: Uuid,
    pub path: &'a str,
    pub content: Option<&'a str>,
}

fn dir_of(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

/// Rebuild the link edges affected by a commit. `head` is the new branch head
/// after the commit; the whole update should run inside the same transaction.
pub async fn after_commit(
    db: &impl ConnectionTrait,
    project_id: Uuid,
    branch_id: Uuid,
    head: Option<Uuid>,
    updates: &[LinkUpdate<'_>],
) -> Result<(), DbErr> {
    if updates.is_empty() {
        return Ok(());
    }

    // Current alive doc set of the branch (after the commit).
    let graph = vcs::CommitGraph::load(db, project_id).await?;
    let states = vcs::branch_doc_states(db, &graph, head).await?;
    let alive_ids: Vec<Uuid> = states
        .values()
        .filter(|s| s.change_type != "deleted")
        .map(|s| s.document_id)
        .collect();

    let mut path_by_id: HashMap<Uuid, String> = HashMap::new();
    if !alive_ids.is_empty() {
        let docs = document::Entity::find()
            .filter(document::Column::ProjectId.eq(project_id))
            .filter(document::Column::Id.is_in(alive_ids))
            .all(db)
            .await?;
        for d in docs {
            path_by_id.insert(d.id, d.path);
        }
    }
    let resolver = PathResolver::new(path_by_id.iter().map(|(id, p)| (p.clone(), *id)));

    // 1) rebuild outgoing edges of updated docs (drop them for deletions)
    for u in updates {
        link_edge::Entity::delete_many()
            .filter(link_edge::Column::BranchId.eq(branch_id))
            .filter(link_edge::Column::SourceDocId.eq(u.doc_id))
            .exec(db)
            .await?;
        let Some(content) = u.content else { continue };
        if !path_by_id.contains_key(&u.doc_id) {
            continue; // not alive on this branch
        }
        let dir = dir_of(u.path);
        for raw in extract_links(content) {
            let target = resolver.resolve(&dir, &raw.raw_target, raw.link_type);
            link_edge::ActiveModel {
                id: Set(Uuid::now_v7()),
                project_id: Set(project_id),
                branch_id: Set(branch_id),
                source_doc_id: Set(u.doc_id),
                target_doc_id: Set(target),
                raw_target: Set(raw.raw_target),
                link_type: Set(raw.link_type.to_string()),
                anchor: Set(raw.anchor),
                display_text: Set(raw.display_text),
                line_no: Set(Some(raw.line_no)),
            }
            .insert(db)
            .await?;
        }
    }

    // 2) edges pointing at deleted docs become dangling
    let deleted: Vec<Uuid> = updates
        .iter()
        .filter(|u| u.content.is_none())
        .map(|u| u.doc_id)
        .collect();
    if !deleted.is_empty() {
        link_edge::Entity::update_many()
            .col_expr(
                link_edge::Column::TargetDocId,
                sea_orm::sea_query::Expr::value(Option::<Uuid>::None),
            )
            .filter(link_edge::Column::BranchId.eq(branch_id))
            .filter(link_edge::Column::TargetDocId.is_in(deleted))
            .exec(db)
            .await?;
    }

    // 3) re-resolve dangling edges (new/renamed docs may satisfy them now)
    let dangling = link_edge::Entity::find()
        .filter(link_edge::Column::BranchId.eq(branch_id))
        .filter(link_edge::Column::TargetDocId.is_null())
        .all(db)
        .await?;
    for e in dangling {
        let Some(src_path) = path_by_id.get(&e.source_doc_id) else {
            // source no longer alive: stale edge, drop it
            link_edge::Entity::delete_by_id(e.id).exec(db).await?;
            continue;
        };
        let dir = dir_of(src_path);
        let resolved = resolver.resolve(&dir, &e.raw_target, &e.link_type);
        if resolved != e.target_doc_id {
            let mut am = e.into_active_model();
            am.target_doc_id = Set(resolved);
            am.update(db).await?;
        }
    }

    Ok(())
}
