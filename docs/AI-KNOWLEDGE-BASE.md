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

### 0.1.2 — P1（下一步）

P1 分四个阶段推进：

- **阶段 1（已完成，0.1.1）**：chunk 级倒排索引 + BM25 + MCP 检索切换
  - 自实现 bigram/词级分词，SQLite / PostgreSQL 通用，不依赖 FTS5 / pg_trgm
  - 按 heading 切块（保留代码块/表格），chunk 带 anchor / 行号
  - `doc_chunks` + `chunk_terms` + `search_index_state`，创建版本时同事务增量索引
  - 启动异步回填历史版本；搜索时对未索引版本懒索引
  - 线上验证：416 个历史版本回填约 15s，3293 chunks / 321k terms；
    MCP 搜索 17–87ms，两字中文词（「住院」「护理」）与长词（「人民医院」）均能命中，
    返回 `section` / `anchor` / 行号；旧内容在更新后不再命中
  - 已知限制：REST 搜索仍走原内存实现（后续统一）；`count` 是本次候选数而非全库总数；
    旧版本的索引暂不回收（GC 待做）；标题高度重复时排序会偏向标题块
- **阶段 2（已完成，0.1.1）**：混合检索（向量 + RRF 融合）
  - 可选 OpenAI 兼容 embedding（Ollama / vLLM / 云端），未配置时自动降级纯 BM25
  - `chunk_vectors` 存 f32 BLOB，应用层余弦计算；不依赖 sqlite-vec / pgvector
  - 后台 worker 每 30s 补齐缺失向量，支持模型变更后重建
  - 检索：BM25 top50 + 向量 top50，RRF（k=60）融合；embedding 失败自动回退
  - 配置：`APP_EMBEDDING__URL`、`APP_EMBEDDING__MODEL`（默认 bge-m3）、
    `APP_EMBEDDING__API_KEY`、`APP_EMBEDDING__BATCH_SIZE`、`APP_EMBEDDING__TIMEOUT_SECS`
  - 已知限制：向量在应用层暴力扫描，适合几千~几万 chunk；更大规模需要 ANN（pgvector / sqlite-vec）；
    新文档向量最多延迟约 30s；REST 搜索仍未切换
- **阶段 3（已完成，0.1.1）**：元数据（front matter 的 tags / description）
  - `doc_meta` + `doc_tags`，创建版本时同步解析；回填补齐历史版本
  - MCP `list_tags` 工具；`search_documents` 支持 `tag` 参数与 `tag:xxx` 查询语法
  - `list_documents` / `read_document` 返回 `tags` 与 `description`
  - REST：`/api/projects/{slug}/tags`、`/api/public/projects/{slug}/tags`；
    文档内容接口返回 tags/description
  - 前端阅读页（工作台 + 公开站点）展示描述与标签
  - front matter 不再进入全文索引，减少标题/标签对正文排序的污染
  - 已知限制：只解析 front matter，不支持行内 `#tag`；前端暂未做按标签筛选文档树
- **阶段 4（已完成，0.1.1；OAuth 除外）**：版本感知与附件
  - `read_document(version_id=...)`（返回 `is_current`）
  - `get_history`（commit、作者、版本 id、当前标记）
  - `get_diff`（默认 head vs 上一版本，也可指定两端）
  - `list_attachments`：文档引用的附件（路径 / URL / MIME / 大小）+ 缺失引用；
    `all=true` 列项目全部附件（要求项目成员）
  - 已知限制：OAuth 2.1 / token scope 尚未实现，仍是静态 Bearer token；
    附件下载路由仍是无鉴权路径，后续可换签名 URL

### 0.1.3 — P2

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

### 后续修复（0.1.1 补丁）

- 审计改为在 `call_tool` 统一记录：成功 `ok=true`，失败 `ok=false` 并写入错误消息；
  工具 panic 时由 `Drop` 兜底记录。
- 审计 / 限流的客户端 IP 在缺少 `X-Forwarded-For` 时回退到 `ConnectInfo`（TCP 对端），
  直连部署也能记录真实来源。
- 全局启用 gzip 压缩（`tower-http::CompressionLayer`）；SSE（`text/event-stream`）
  由默认谓词自动排除，不影响 MCP 流式响应。

### 0.1.1 补丁二：resources 审计与限流响应头

- `list_resources` / `read_resource` 接入统一的 `ToolAudit`：
  - 失败写入 `ok=false` + 错误消息（以前只记成功）
  - `read_resource` 从 `mdocs://{project}/{path}` 解析出 `project`，便于按项目筛选
- 限流响应头：
  - 所有 `/mcp` 响应带 `RateLimit-Limit`、`RateLimit-Remaining`、`RateLimit-Reset`
  - 429 额外带 `Retry-After`，JSON 体包含 `retry_after_seconds`
- 验证：本地与线上均确认 `resources/read` 失败记录为 `ok=f`，且 `project` 正确。
