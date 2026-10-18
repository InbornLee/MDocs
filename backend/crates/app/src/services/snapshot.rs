//! 分支快照缓存。
//!
//! 一个 `(project, branch, head_commit)` 组合对应一份不可变的文档视图：
//! commit 与 `doc_versions` 一旦写入就不再变化，因此只要 head 不变，快照
//! 就永远有效。`documents.published` 可以在不产生 commit 的情况下单独切换，
//! 所以它不进入快照，读取时用 [`published_ids`] 做一次轻量过滤。

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::entities::{doc_version, document, link_edge};
use crate::error::AppResult;
use crate::services::vcs;
use crate::state::AppState;

pub type SnapshotKey = (Uuid, Uuid, Option<Uuid>);

/// 快照里的一篇文档（含全文）。
#[derive(Debug, Clone)]
pub struct DocSnapshot {
    pub id: Uuid,
    pub path: String,
    pub title: String,
    pub size_bytes: i64,
    pub updated_at: DateTime<Utc>,
    #[allow(dead_code)] // 后续版本读取 / 增量索引会用到
    pub version_id: Option<Uuid>,
    pub content: String,
}

/// 快照里的一条链接边（已按分支过滤）。
#[derive(Debug, Clone)]
pub struct EdgeSnapshot {
    pub source_doc_id: Uuid,
    pub target_doc_id: Option<Uuid>,
    pub raw_target: String,
    pub link_type: String,
    pub anchor: Option<String>,
}

/// 某个分支 head 的完整只读视图。
#[derive(Debug)]
#[allow(dead_code)] // 标识字段保留给日志 / 后续增量索引使用
pub struct Snapshot {
    pub project_id: Uuid,
    pub branch_id: Uuid,
    pub head_commit_id: Option<Uuid>,
    /// 按 path 升序
    pub docs: Vec<DocSnapshot>,
    pub edges: Vec<EdgeSnapshot>,
}

impl Snapshot {
    pub fn doc_by_path(&self, path: &str) -> Option<&DocSnapshot> {
        self.docs.iter().find(|d| d.path == path)
    }

    pub fn doc_by_id(&self, id: Uuid) -> Option<&DocSnapshot> {
        self.docs.iter().find(|d| d.id == id)
    }

    /// 路径容忍缺少 `.md` / `.markdown` 后缀，与 read_document 的历史行为一致。
    pub fn find_doc(&self, path: Option<&str>, id: Option<Uuid>) -> Option<&DocSnapshot> {
        if let Some(id) = id {
            if let Some(d) = self.doc_by_id(id) {
                return Some(d);
            }
        }
        let path = path?;
        let path = path.trim().trim_start_matches("./");
        self.doc_by_path(path).or_else(|| {
            if path.ends_with(".md") || path.ends_with(".markdown") {
                None
            } else {
                self.doc_by_path(&format!("{path}.md"))
                    .or_else(|| self.doc_by_path(&format!("{path}.markdown")))
            }
        })
    }
}

#[derive(Default)]
struct CacheInner {
    map: HashMap<SnapshotKey, Arc<Snapshot>>,
    order: VecDeque<SnapshotKey>,
}

/// 极简 LRU：容量很小（只保留最近若干个 head），不需要引入额外依赖。
pub struct SnapshotCache {
    inner: Mutex<CacheInner>,
    capacity: usize,
}

impl SnapshotCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(CacheInner::default()),
            capacity: capacity.max(1),
        }
    }

    pub fn get(&self, key: &SnapshotKey) -> Option<Arc<Snapshot>> {
        let inner = self.inner.lock().ok()?;
        inner.map.get(key).cloned()
    }

    pub fn put(&self, key: SnapshotKey, value: Arc<Snapshot>) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if inner.map.insert(key, value).is_none() {
            inner.order.push_back(key);
        }
        while inner.order.len() > self.capacity {
            if let Some(old) = inner.order.pop_front() {
                inner.map.remove(&old);
            }
        }
    }
}

async fn build(
    state: &AppState,
    project_id: Uuid,
    branch_id: Uuid,
    head: Option<Uuid>,
) -> AppResult<Snapshot> {
    let graph = vcs::CommitGraph::load(&state.db, project_id).await?;
    let states = vcs::branch_doc_states(&state.db, &graph, head).await?;

    let mut version_of: HashMap<Uuid, Uuid> = HashMap::new();
    for st in states.values() {
        if st.change_type != "deleted" {
            if let Some(v) = st.version_id {
                version_of.insert(st.document_id, v);
            }
        }
    }

    let alive: Vec<Uuid> = version_of.keys().copied().collect();
    let mut docs = Vec::new();
    if !alive.is_empty() {
        let rows = document::Entity::find()
            .filter(document::Column::ProjectId.eq(project_id))
            .filter(document::Column::Id.is_in(alive))
            .all(&state.db)
            .await?;

        let version_ids: Vec<Uuid> = version_of.values().copied().collect();
        let versions: HashMap<Uuid, doc_version::Model> = doc_version::Entity::find()
            .filter(doc_version::Column::Id.is_in(version_ids))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|v| (v.id, v))
            .collect();

        docs = rows
            .into_iter()
            .map(|d| {
                let vid = version_of.get(&d.id).copied();
                let ver = vid.and_then(|v| versions.get(&v));
                DocSnapshot {
                    id: d.id,
                    path: d.path,
                    title: d.title,
                    size_bytes: ver.map(|v| v.size_bytes as i64).unwrap_or(0),
                    updated_at: d.updated_at,
                    version_id: vid,
                    content: ver.map(|v| v.content.clone()).unwrap_or_default(),
                }
            })
            .collect();
        docs.sort_by(|a, b| a.path.cmp(&b.path));
    }

    let edges = link_edge::Entity::find()
        .filter(link_edge::Column::BranchId.eq(branch_id))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|e| EdgeSnapshot {
            source_doc_id: e.source_doc_id,
            target_doc_id: e.target_doc_id,
            raw_target: e.raw_target,
            link_type: e.link_type,
            anchor: e.anchor,
        })
        .collect();

    Ok(Snapshot {
        project_id,
        branch_id,
        head_commit_id: head,
        docs,
        edges,
    })
}

/// 取（或构建）分支 head 的快照。head 为 `None`（空分支）时返回空快照。
pub async fn load(
    state: &AppState,
    project_id: Uuid,
    branch_id: Uuid,
    head: Option<Uuid>,
) -> AppResult<Arc<Snapshot>> {
    let key = (project_id, branch_id, head);
    if let Some(hit) = state.snapshots.get(&key) {
        return Ok(hit);
    }
    let snap = Arc::new(build(state, project_id, branch_id, head).await?);
    state.snapshots.put(key, snap.clone());
    Ok(snap)
}

/// 项目下所有 `published = true` 的文档 id。
///
/// 单独查询而不是放在快照里：发布状态可以在不产生 commit 的情况下切换。
pub async fn published_ids(state: &AppState, project_id: Uuid) -> AppResult<HashSet<Uuid>> {
    Ok(document::Entity::find()
        .filter(document::Column::ProjectId.eq(project_id))
        .filter(document::Column::Published.eq(true))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|d| d.id)
        .collect())
}
