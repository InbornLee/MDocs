# MDocs

[中文](README.md) · [English](README.en.md)

> **单二进制、AI 原生的 Markdown 知识库** —— 像 Git 一样管理版本，像搜索引擎一样检索。

MDocs 把散落的 Markdown 变成可协作、可追溯、可被 AI 直接使用的知识库：

- **轻量**：一个 Rust 二进制同时提供 SPA、REST API 与 MCP 端点；SQLite 默认，无 Node / Python / JVM 运行时。
- **AI 原生**：MCP Streamable HTTP 内置 11 个工具、Resources 与引用提示词，检索结果自带 URL、锚点、行号与版本。
- **检索优先**：中文 bigram + BM25 倒排索引，可选向量混合排序（RRF）；按标题分块，支持按节精读，而不是把整篇文档塞给模型。

当前版本 **0.1.1**。检索与 AI 能力的评估、路线和取舍见 [docs/AI-KNOWLEDGE-BASE.md](docs/AI-KNOWLEDGE-BASE.md)。

---

## 为什么是 MDocs

| | 说明 |
| --- | --- |
| **单二进制** | SPA 通过 `rust-embed` 内嵌，静态资源、REST API、MCP 由同一进程提供；没有独立的前端服务、反向代理或 worker。 |
| **零外部依赖启动** | 默认 SQLite + 本地文件存储，`docker run` 即可用；需要时再切 PostgreSQL，embedding 也是可选项。 |
| **镜像小** | 运行时基于 Debian slim，镜像约 46 MB（压缩层）；不含 Node、Python、JVM。 |
| **AI 直接可用** | 不需要额外网关或插件，AI 客户端连上 `/mcp` 就能检索、精读、查历史、看附件。 |
| **可解释的检索** | 每次命中都能回溯到文档路径、标题、锚点、行号和版本，而不是只给一段相似文本。 |
| **国内源构建** | Docker 构建默认走 apt / npm / crates.io 国内镜像，可按需覆盖。 |

---

## 核心能力

### 文档与协作

- 树形目录、在线编辑（CodeMirror 6）、附件与图片、全文检索
- 分支、commit 历史、任意版本回溯、revert、两版本 diff
- 行级三方合并；分支合并走 MR 流程，支持预览、评审与冲突解决
- 项目成员与角色（拥有者 / 编辑者 / 只读）
- 公开站点：项目与单篇文档可分别发布

### 元数据与链接

- 解析 front matter 的 `tags` / `description`，阅读页展示，检索支持 `tag:` 过滤
- 自动解析相对链接 `[text](./path/doc.md)` 与 `[[wiki]]` / `[[name|显示名]]`
- ECharts 力导向链接图，悬空链接红色虚线标识；重命名时同步更新引用

### 导入导出

- 支持 `.zip` 文档库、单个 `.md`、tree JSON 导入
- 导出 zip / tree JSON / 单文档；兼容中文与 GBK 文件名的压缩包

### 部署

- 默认 SQLite，开箱即用；可选 PostgreSQL
- 单容器部署；数据库迁移启动时自动执行
- 构建默认使用国内镜像

---

## 检索是怎么工作的

MDocs 的检索不是“把全文丢进 LIKE”，而是为 AI 使用场景设计的四层结构：

### 1. 分词：不依赖扩展，也能检索中文

- 中文按 **bigram**（「人民医院」→ 人民 / 民医 / 医院），单字兜底
- 英文与数字按单词、大小写归一
- 不依赖 SQLite FTS5、PostgreSQL `pg_trgm` 或任何中文分词器，SQLite / PostgreSQL 行为一致

### 2. 索引：按标题分块，按版本增量

- 文档按 heading 切块，保留代码块与表格；超大 section 再按行/字节切分
- `doc_chunks` 存块文本、标题、锚点、行范围；`chunk_terms` 存 `(term, tf)` 倒排
- 索引按 `doc_versions` 建立（版本不可变），搜索时用当前分支 head 的版本集合过滤
- 创建版本时**同事务**写入索引；启动时后台回填历史版本；搜索时对未索引版本懒索引

### 3. 召回与排序：BM25 可解释打分

- 倒排表按 `SUM(tf)` 粗排候选 chunk
- 应用层 BM25 精排（k1=1.2, b=0.75），标题命中、路径命中再叠加权重
- 结果按 `(文档, section)` 去重，返回 `score`、`section`、`anchor`、`line_start` / `line_end`

### 4. 混合检索：BM25 + 向量，失败自动降级

- 可选 OpenAI 兼容 embedding（Ollama / vLLM / 云端）
- `chunk_vectors` 存 f32 BLOB，应用层余弦相似度；不依赖 sqlite-vec / pgvector
- BM25 top50 + 向量 top50，按 **RRF（k=60）** 融合；embedding 超时或未配置时自动回退纯 BM25
- 后台 worker 每 30 秒补齐缺失向量，支持模型变更后重建

### 5. 上下文：让模型读“该读的那一段”

- `get_document_outline` 返回标题树与锚点
- `read_document(section=...)` 只返回该节；`offset` + `limit` 按行读取
- 响应带 `total_lines` / `total_bytes` / `truncated`，避免上下文悄然爆掉
- 搜索结果直接携带 `url#anchor`，模型回答时可以给出可点击引用

### 6. 规模与实测

- 示例数据集：416 个历史版本、3,293 个 chunk、321k 条倒排记录
- 历史索引回填约 15 秒；单次检索约 **10–90ms**（数千 chunk 规模）
- 向量检索目前在应用层扫描，适合几千到几万 chunk；更大规模可接 SQLite-vec / pgvector ANN

---

## 快速开始

### Docker Compose（推荐）

仓库根目录的 `docker-compose.yml` 默认连接**宿主机上的外部 PostgreSQL**，并挂载 `mdocs-data` 卷保存上传文件。

1. 准备数据库（二选一）：

   ```bash
   # A. 使用外部 PostgreSQL（compose 默认，需在宿主机建库）
   #    连接串：postgresql://mdoc:mdoc@host.docker.internal:5432/mdoc
   createdb mdoc

   # B. 改用内置 SQLite：编辑 docker-compose.yml，把 APP_DATABASE__URL 设为
   #    sqlite:///data/app.db?mode=rwc
   ```

2. 设置密钥并启动：

   ```bash
   cp .env.example .env
   # 编辑 .env，至少设置 APP_JWT__SECRET（生成方式：openssl rand -hex 32）
   docker compose up -d --build
   ```

3. 访问 <http://localhost:8008>（主机端口可用 `MDOCS_PORT` 调整）。

> 数据库迁移在启动时自动执行，首次运行无需手工建表。

### 本地开发

前置：Rust 1.97+、Node.js 20.19+（Docker 镜像使用 Node 24）。

```bash
# 1) 后端：监听 8097（与前端 vite 代理约定一致）
cd backend
APP_LISTEN=0.0.0.0:8097 APP_JWT__SECRET=dev-secret cargo run -p app
# 默认 SQLite：./backend/data/app.db

# 2) 前端：开发服务器 5173，代理 /api /mcp /uploads 到 8097
cd frontend
npm install
npm run dev
```

访问 <http://localhost:5173>。

**构建生产前端**（输出到 `backend/crates/app/frontend-dist/`，供 `rust-embed` 内嵌）：

```bash
cd frontend && npm run build:check
cd ../backend && cargo build --release -p app
# 产物：target/release/mdocs
```

---

## 配置项

后端通过环境变量配置（`backend/crates/app/src/config.rs`）：

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `APP_LISTEN` | `0.0.0.0:8080` | 监听地址 |
| `APP_DATA_DIR` | `./data` | 数据目录（SQLite 数据库与上传文件） |
| `APP_DATABASE__URL` | `sqlite://<data_dir>/app.db?mode=rwc` | 数据库连接串，支持 `sqlite://` 与 `postgres://` |
| `APP_JWT__SECRET` | 随机临时值 | JWT 签名密钥；未设置时每次重启会使会话失效 |
| `APP_UPLOAD__MAX_AVATAR_MB` | `5` | 头像上传大小上限（MB） |
| `APP_PUBLIC_BASE_URL` | 空 | 对外站点地址，设置后 MCP 返回绝对阅读链接 |
| `APP_MCP__ALLOWED_HOSTS` | 空 | MCP 端点额外允许的 Host（逗号分隔，容器 / 反向代理场景需要） |
| `APP_MCP__MAX_CONTENT_BYTES` | `65536` | `read_document` 单次返回的字节上限（超出截断并标记 `truncated`） |
| `APP_MCP__RATE_LIMIT_PER_MIN` | `240` | `/mcp` 每分钟请求上限（按 token / IP），`0` 关闭 |
| `APP_MCP__SNAPSHOT_CACHE` | `8` | 分支快照缓存保留的 head 数量 |
| `APP_EMBEDDING__URL` | 空 | OpenAI 兼容的 embedding 端点（如 Ollama `/v1/embeddings`）；空则纯 BM25 |
| `APP_EMBEDDING__MODEL` | `bge-m3` | embedding 模型名 |
| `APP_EMBEDDING__API_KEY` | 空 | embedding 服务的 Bearer token |
| `APP_EMBEDDING__BATCH_SIZE` | `16` | 每批送模型的 chunk 数 |
| `APP_EMBEDDING__TIMEOUT_SECS` | `30` | embedding 请求超时 |
| `RUST_LOG` | `info` | 日志级别 |

Docker 构建参数（默认已指向国内镜像，传空字符串可回退上游）：

| 参数 | 默认值 |
| --- | --- |
| `APT_MIRROR` | `http://mirrors.aliyun.com` |
| `NPM_REGISTRY` | `https://registry.npmmirror.com` |
| `CARGO_REGISTRY_MIRROR` | `sparse+https://rsproxy.cn/index/` |

---

## 接入 AI（MCP）

MCP 端点与 REST API **同端口**，路径为 `/mcp`（Streamable HTTP）。

### 工具

| 工具 | 说明 |
| --- | --- |
| `list_projects` | 列出文档项目（默认仅公开项目），含默认分支与文档数 |
| `list_branches` | 列出项目分支 |
| `list_documents` | 文档路径、标题、大小、标签与阅读链接 |
| `list_tags` | front matter 标签及文档数 |
| `get_document_outline` | 标题树（level / title / anchor / 行号） |
| `read_document` | 读取全文或指定 `version_id`；支持 `section` / `offset`+`limit`，返回 `is_current` / `truncated` |
| `search_documents` | BM25 + 可选向量混合检索；返回 `score`、`section`、`anchor`、`url`，支持跨项目、分页与 `tag` 过滤 |
| `get_link_graph` | 文档间有向链接图谱 |
| `get_history` | 版本历史（commit、作者、版本 id、是否当前版本） |
| `get_diff` | 两个版本之间的 unified diff |
| `list_attachments` | 文档引用的附件（路径 / URL / MIME / 大小），`all=true` 列项目全部附件 |

### Resources 与 Prompt

- **Resources**：`mdocs://{project}/{path}`（含资源模板），客户端可直接按 URI 拉取文档
- **Prompt**：`answer_with_citations`，引导模型先检索、按节精读，回答时附文档链接与锚点

### 权限与治理

- 匿名只能访问**公开项目**与已发布文档
- 在「设置 → API Token」生成令牌（可设置有效期），通过 `Authorization: Bearer <token>` 访问自己作为成员的项目
- 服务端按 token / IP 做分钟级限流，429 带 `Retry-After` 与 `RateLimit-*`
- 每次工具调用写入 `mcp_audit_log`：工具、项目、参数摘要、调用者、IP、成功/失败

### 客户端配置

```json
{
  "mcpServers": {
    "mdocs": {
      "type": "streamable-http",
      "url": "http://localhost:8008/mcp"
    }
  }
}
```

本地调试可使用 MCP Inspector：`npx @modelcontextprotocol/inspector`，连接 `http://localhost:8080/mcp`。

---

## 文档写作

项目支持两种链接写法，保存时自动解析并更新链接树：

```markdown
[相对链接](./guide/install.md)
[带标题](../api/auth.md#登录)
[[wiki 链接]]
[[target-doc|显示名称]]
```

front matter 的标签与描述会进入元数据索引，但不会污染正文检索：

```markdown
---
tags: [his, 住院]
description: 医院核心业务系统文档
---
```

- 指向不存在的目标会生成「悬空链接」，在链接树中以红色虚线展示
- 重命名文档时同步更新引用
- Markdown 渲染默认关闭内联 HTML（`html: false`）以防 XSS，后端不渲染 HTML

---

## 技术栈

| 层 | 技术 |
| --- | --- |
| 后端 | Rust · axum 0.8 · tokio · SeaORM 1.1（SQLite / PostgreSQL） · rmcp 3.3（MCP） · comrak（Markdown AST） · similar（diff / 三方合并） · tower-http（压缩 / 限流） · jsonwebtoken · argon2 |
| 检索 | 自实现 bigram/词级分词 · 倒排表 + BM25 · 可选 OpenAI 兼容 embedding + RRF 融合 |
| 前端 | Vue 3.5 · Vite 8 · TypeScript · Naive UI · Pinia · Vue Router · CodeMirror 6 · markdown-it · highlight.js · ECharts · Mermaid |
| 部署 | Docker 三阶段构建（Node 构建 SPA → Rust 编译内嵌前端 → Debian slim 运行时），单容器提供 SPA + REST API + MCP |

---

## 项目结构

```
mdocs-main/
├── backend/
│   ├── Dockerfile                 # 三阶段构建：frontend → backend → runtime
│   ├── Cargo.toml                 # workspace
│   └── crates/
│       ├── migration/             # SeaORM 迁移（全部表结构）
│       └── app/
│           ├── src/
│           │   ├── main.rs        # 配置加载 → 连库 → 迁移 → Router（API + /mcp + /uploads + SPA fallback）
│           │   ├── config.rs  state.rs  error.rs  mcp.rs
│           │   ├── entities/      # SeaORM 实体
│           │   ├── routes/        # auth user project document member mr transfer public admin
│           │   └── services/      # vcs / merge / links / index / search_index / metadata / embedding / snapshot / audit / rate_limit 等
│           └── frontend-dist/     # 前端构建产物（vite 生成，已 gitignore）
├── frontend/
│   ├── src/
│   │   ├── api/  stores/  router/  components/  utils/  content/
│   │   └── views/                 # 首页 / 登录注册 / 项目工作台 / 公开站点 / 管理后台
│   └── vite.config.ts             # 开发代理 → 8097，构建输出 → backend/.../frontend-dist
├── docker-compose.yml
├── docs/                          # 评估与改进路线（AI-KNOWLEDGE-BASE.md）
└── README.md
```

---

## 主要 REST 端点

除公开接口外均需 `Authorization: Bearer <JWT>`。

- **认证 / 用户**：`POST /api/auth/register`、`POST /api/auth/login`、`GET /api/auth/me`、`PATCH /api/me/profile`、`POST /api/me/avatar`
- **项目 / 成员**：`GET|POST /api/projects`、`GET|PATCH|DELETE /api/projects/{slug}`、成员 CRUD
- **分支 / 文档**：`/api/projects/{slug}/branches`、`tree`、`docs`（保存携带 `base_version_id` 乐观锁，冲突返回 409）、`tags`
- **历史**：`docs/{doc_id}/history`、`versions/{version_id}`、`revert`、`commits/{id}`、`diff`
- **导入导出**：`import`、`export.zip`、`export.tree`、`docs/{doc_id}/export`
- **合并请求**：`mrs`、`merge-preview`、`reviews`、`merge`、`close`
- **链接树**：`branches/{branch}/link-tree`
- **附件**：`/uploads/attachments/{project_id}/{path}`（公开项目匿名可读；私有项目需成员 JWT / API token）
- **公开**：`/api/public/...`（项目、文档、搜索、历史、标签、链接）
- **健康检查 / 版本**：`GET /api/health`、`GET /api/version`

---

## 路线

- **0.1.0**：首个版本（文档、版本控制、MR、链接树、公开站点、基础 MCP）
- **0.1.1**：AI 知识库 P0 + P1
  - chunk 倒排索引 + BM25
  - 可选向量混合检索（RRF）
  - front matter 标签 / 描述
  - 版本读取、历史、diff、附件清单
  - MCP 审计、限流、token 有效期、附件访问鉴权
- **待做**：OAuth 2.1 / token scope、附件签名 URL、ANN 索引、内置 `ask` 与知识质量治理

详见 [docs/AI-KNOWLEDGE-BASE.md](docs/AI-KNOWLEDGE-BASE.md)。

## License

[MIT](LICENSE) © 2026 Inborn Lee (李颖博)

<br>

<p align="center"><sub>MDocs · 单二进制、AI 原生的 Markdown 知识库</sub></p>
