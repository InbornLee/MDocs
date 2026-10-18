# MDocs 作为 AI 文档知识库：评估与改进路线

> 本文是 2026-10 对 MDocs 的 AI 知识库能力评估结论、分阶段改进清单与版本计划。
> 评估基线：0.1.0；P0 落地后：0.1.1；P1（检索索引与混合检索）：0.2.0。

## 1. 结论

MDocs 已经是一个**可直接接入 AI 客户端的文档知识库**：MCP Streamable HTTP 端点 `/mcp` 与 REST API 同进程，
只读、按项目可见性鉴权，读的是 Markdown 源文，并额外提供链接图谱。
它适合「小到中等规模、中文为主、需要项目/分支/发布状态隔离」的场景；
短板集中在**检索质量、上下文工程与治理**，而不是数据模型。

## 2. 优势

| 优势 | 说明 |
| --- | --- |
| MCP 原生内置 | `backend/crates/app/src/mcp.rs`，`/mcp` 与 REST 同端口、同进程，Streamable HTTP |
| 权限模型复用公开站 | 匿名仅 public 项目 + published 文档；`Bearer mdocs_...` 按项目角色放开未发布内容 |
| 返回 Markdown 源文 | 标题、表格、代码块、链接语法完整，最适合 LLM 阅读 |
| 链接图谱落库 | `link_edges` 按分支存有向边，可直接给出 in/out/dangling 与孤立节点 |
| 版本与分支语义 | 每次保存生成不可变 `doc_versions` + commit，AI 可指定分支读取 |
| 内容即状态 | 无独立索引，改完即被 AI 看到（代价见短板） |
| 部署极轻 | 单二进制、SQLite 默认、`docker compose up` 即用 |

## 3. 短板（0.1.0 基线）

1. **检索是内存子串匹配**：无倒排索引 / BM25 / 向量；排序按 `occurrences`，不是相关度；
   每次请求把所有文档全文加载进内存（`services/search.rs`、`mcp.rs::docs_with_content`）。
2. **无缓存**：`CommitGraph::load` + `branch_doc_states` 在每次工具调用时重新扫描全项目 commit 历史。
3. **上下文粒度过粗**：`read_document` 只能整篇返回，无 outline、无按节读取、无截断提示。
4. **结果不可溯源**：search 只返回 path/snippet，没有 URL、anchor、commit、更新时间。
5. **治理缺位**：无 MCP 调用审计、无限流、API token 无过期。
6. **附件不可见**：图片/附件只存磁盘，MCP 不暴露。
7. **工程欠账**：无自动化测试；compose 中 JWT 密钥有弱默认值。

## 4. 版本计划

### 0.1.0 — 基线

初始版本，作为评估对象。

### 0.1.1 — P0（本次）

目标：**让 AI 拿到的每条结果都能定位、能分段读、能跨项目检索，并且不把服务打挂。**

- [x] 检索结果补 citation：`url`、`anchor`、`commit_id`、`updated_at`、`score`
- [x] 相关度排序：标题 > 路径 > 正文，结合命中次数与首次命中位置
- [x] 分支快照缓存：按 `(project, branch, head_commit)` 缓存文档元数据、全文与链接边
- [x] `get_document_outline`：返回标题树（level / title / slug / 行号）
- [x] `read_document` 支持 `section` / `offset` / `limit`，返回 `total_bytes` / `truncated`
- [x] `list_branches` 工具
- [x] `search_documents` 支持 `offset` 与跨项目检索（`projects`）
- [x] MCP Resources：`mdocs://{project}/{path}` 与资源模板（`resources/list`、`resources/templates/list`、`resources/read`）
- [x] MCP Prompt：`answer_with_citations`
- [x] MCP 调用审计表 `mcp_audit_log`
- [x] API token 过期时间（`expires_at`）
- [x] MCP 速率限制（按 token / IP，每分钟）
- [x] `/api/version` 与前端版本显示

实现说明：

- 快照缓存 key 为 `(project_id, branch_id, head_commit_id)`，默认保留 8 个 head，可通过
  `APP_MCP__SNAPSHOT_CACHE` 调整；`published` 状态不进入快照，每次用一次轻量查询过滤。
- 默认单次 `read_document` 返回上限 64KB（`APP_MCP__MAX_CONTENT_BYTES`），超出截断并置
  `truncated=true`；`section` / `offset` 读取同样受该上限保护。
- 限流默认 240 次/分钟（`APP_MCP__RATE_LIMIT_PER_MIN`，0 关闭），key 为 `IP + token 摘要`。
- 审计表 `mcp_audit_log` 只记录工具名、项目、参数摘要、调用者与结果，不落文档正文。
- token 默认永不过期，创建时可指定 7 / 30 / 90 天；过期后 MCP 返回明确的
  `API token expired` 错误。

验收标准：

- 同一 `head_commit` 下重复调用 `list_documents` / `search_documents` 不再重复扫描 commit 历史；
- 搜索结果按 `score` 降序，带可访问 URL 与 heading anchor；
- 大文档可按 section 读取，响应体不超过配置上限并显式标记 `truncated`；
- 过期 token 返回明确错误；超过速率上限返回 429；每次工具调用在审计表留痕。

### 0.2.0 — P1（下一步）

- 倒排索引：SQLite FTS5（trigram，中文友好）/ PostgreSQL `tsvector + pg_trgm`，增量维护在 commit 钩子
- Chunk 级索引与检索（按 heading 切块，保留代码块/表格）
- 混合检索：BM25 + 向量，RRF 融合
- 元数据：front matter 的 tags / description 入索引，`list_tags` 与按 tag 过滤
- 版本感知：`read_document(version=...)`、`get_history`、`get_diff`
- 附件：MCP 返回引用附件清单与可访问 URL，图片可做 OCR/描述
- 鉴权：OAuth 2.1 / 动态客户端注册，token scope

### 0.3.0 — P2

- 内置 `ask`（带引用、可拒答）
- 知识质量：死链、孤立文档、语义重复/冲突检测
- 多模态与可观测性：图片理解、Prometheus 指标、压测与测试补齐

## 5. pgvector 评估

**结论：0.1.1 不引入 pgvector；0.2.0 作为可选的检索后端，而不是强制依赖。**

### 5.1 为什么现在不需要

- 默认部署是 SQLite（单文件、单容器），pgvector 只存在于 PostgreSQL 生态，
  引入它等于把「默认可用」变成「必须准备带扩展的 PG」。
- 当前检索瓶颈是**没有索引**，不是「缺语义」。FTS5（trigram）或 `tsvector + pg_trgm`
  就能把字面量检索从 O(N×M) 全表扫描降到索引查找，成本远低于向量化。
- 向量检索需要 embedding 生产链路（模型、维度、批量任务、重建策略），
  在 0.1.x 阶段属于过度设计；对「几千篇以内」的文档库，BM25 类检索的召回通常够用。
- 嵌入向量会随模型升级而变化，必须版本化 + 重建；这需要索引任务与存储设计先到位。

### 5.2 需要 pgvector 的触发条件

满足任意两条再考虑：

1. 文档量 > 5,000 篇，或单项目 > 500 篇且查询频繁；
2. 用户提问是自然语言长句，字面量召回明显不足（同义改写、跨语言）；
3. 需要「相似文档 / 语义重复检测」，而不只是关键词命中；
4. 已有生产级 PostgreSQL（>= 13）且允许 `CREATE EXTENSION vector`；
5. 能承担 embedding 的算力与存储（例如 1024 维 × 10 万 chunk ≈ 400MB 原始向量）。

### 5.3 0.2.0 的建议架构

```
chunk 表（跨后端统一）:
  id, document_id, version_id, branch_id, ord,
  heading, anchor, text, token_count, content_hash

检索抽象 SearchBackend:
  - SqliteFts     : FTS5(trigram)  + sqlite-vec（可选）
  - PgFts         : tsvector/pg_trgm
  - PgVector      : pgvector HNSW（可选，需扩展）
```

- 迁移中**不要**直接 `CREATE EXTENSION`：单独的可选迁移，失败时降级到 FTS 后端并记录日志。
- 向量列与维度绑定，必须提供 `embedding_model` 与 `dim` 的元数据表，模型变更时整表重建。
- 混合排序用 RRF（`1/(k+rank)`）融合字面量与向量结果，避免两套分数量纲不一致。
- 若最终只服务单机小团队，SQLite + FTS5 + 本地小模型（fastembed/ONNX）足够，无需 pgvector。

## 6. 关键实现位置

| 能力 | 文件 |
| --- | --- |
| MCP 工具 / resources / prompts | `backend/crates/app/src/mcp.rs` |
| 分支快照缓存 | `backend/crates/app/src/services/snapshot.rs` |
| 标题与分节 | `backend/crates/app/src/services/heading.rs` |
| 检索匹配与评分 | `backend/crates/app/src/services/search.rs` |
| MCP 审计 | `backend/crates/app/src/services/audit.rs` |
| MCP 限流 | `backend/crates/app/src/services/rate_limit.rs` |
| token 过期 | `backend/crates/app/src/services/api_token.rs` |

## 7. 0.1.1 验证记录

- `cargo test -p app`：25 个测试通过（新增 heading 的 slug / front matter / 分节 / 截断测试）。
- `npm run build:check`：`vue-tsc --noEmit` + `vite build` 通过。
- 端到端（SQLite + 本地二进制）：
  - `GET /api/version` 返回 `0.1.1` 与 7 个工具；
  - MCP `initialize` / `tools/list` / `tools/call` / `resources/list` / `resources/read` /
    `prompts/list` / `prompts/get` 全部正常；
  - `search_documents` 返回 `score`、`anchor`（如 `镜像源`）、`url`、`commit_id`；
  - `read_document(section="安装步骤")` 只返回该节（13-20 行），front matter 不再被当成标题；
  - 过期 token 返回 `API token expired`；限流设为 2/分钟时第 3 次请求返回 429；
  - `mcp_audit_log` 中可查到 `list_projects` / `search_documents` / `read_resource` 等记录。
