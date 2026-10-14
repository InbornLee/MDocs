# MDocs

> 类 Git 的 Markdown 文档集分类管理系统 —— 集中管理 · 自动链接树 · 协作版本管理 · AI 知识库。

MDocs 把散落的 Markdown 文档变成可协作、可追溯的知识库：像 Git 一样管理文档版本与分支，自动解析文档间的相对链接与 `[[wiki]]` 链接生成关系图谱，并通过 MCP 端点把已发布的文档直接暴露给 AI 客户端。

前后端全部由**单个 Rust 二进制**提供（SPA 通过 `rust-embed` 内嵌），一次构建、单容器部署。

---

## 功能特性

- **文档管理**：树形目录浏览、在线编辑（CodeMirror 6 + Markdown 语法高亮）、附件与图片、按关键词全文搜索。
- **版本控制（类 Git）**：分支、commit 链、历史时间线、任意版本回溯、版本恢复（revert）、两 commit 差异对比。
- **行级三方合并**：基于 `similar` 的三路合并，自动合并无冲突行，冲突块以 Git 风格标记，前端提供「保留我方 / 采用对方 / 手动编辑」三选一解决界面。
- **合并请求（MR）**：分支合并走 MR 流程，支持预览合并结果、评审（approved / rejected / commented）与冲突解决。
- **自动链接树**：解析标准相对链接 `[text](./path/doc.md)` 与 Obsidian 风格 `[[wiki]]` / `[[name|显示名]]`，用 ECharts 力导向图展示文档关系，悬空链接以红色虚线标识。
- **导入导出**：支持 `.zip` 文档库、单个 `.md`、tree JSON 导入；导出为 zip / tree JSON / 单文档下载。兼容中文（含 GBK 文件名）压缩包。
- **公开站点与发布**：项目可设为公开、文档可单独发布，未登录访客可浏览公开内容，支持自定义站点信息。
- **AI 知识库（MCP）**：内置 MCP Streamable HTTP 端点，AI 客户端可直接检索文档、读取全文、获取链接图谱；支持用 API Token 访问私有项目。
- **多数据库**：默认 SQLite，开箱即用；也支持 PostgreSQL。
- **国内源构建**：Docker 构建默认走 apt / npm / crates.io 国内镜像，可按需覆盖。

---

## 技术栈

| 层 | 技术 |
| --- | --- |
| 后端 | Rust · axum 0.8 · tokio · SeaORM 1.1（SQLite / PostgreSQL） · rmcp 3.2（MCP） · comrak（Markdown AST） · similar（diff / 三方合并） · jsonwebtoken · argon2 |
| 前端 | Vue 3.5 · Vite 8 · TypeScript · Naive UI · Pinia · Vue Router · CodeMirror 6 · markdown-it · highlight.js · ECharts · Mermaid |
| 部署 | Docker 三阶段构建（Node 构建 SPA → Rust 编译内嵌前端 → Debian 运行时），单容器提供 SPA + REST API + MCP |

---

## 快速开始

### 方式一：Docker Compose（推荐）

仓库根目录的 `docker-compose.yml` 默认连接**宿主机上的外部 PostgreSQL**，并挂载 `mdocs-data` 卷保存上传文件。

1. 准备数据库（二选一）：

   ```bash
   # A. 使用外部 PostgreSQL（compose 默认，需在宿主机建库）
   #    连接串：postgresql://mdoc:mdoc@host.docker.internal:5432/mdoc
   createdb mdoc

   # B. 改用内置 SQLite：编辑 docker-compose.yml，把 APP_DATABASE__URL 改为
   #    留空或设置成 sqlite:///data/app.db?mode=rwc
   ```

2. 设置密钥并启动：

   ```bash
   # APP_JWT__SECRET 用于签发会话，务必改成足够长的随机串
   export APP_JWT__SECRET="$(openssl rand -hex 32)"

   docker compose up -d --build
   ```

3. 访问 <http://localhost:8008>（主机端口可用 `MDOCS_PORT` 环境变量调整）。

> 数据库迁移在后端启动时自动执行，首次运行无需手工建表。

### 方式二：本地开发

前置：Rust 1.97+、Node.js 20.19+（Docker 镜像使用 Node 24）。

```bash
# 1) 后端：监听 8097（与前端 vite 代理约定一致）
cd backend
APP_LISTEN=0.0.0.0:8097 APP_JWT__SECRET=dev-secret cargo run -p app
# 默认使用 SQLite：./backend/data/app.db

# 2) 前端：开发服务器 5173，自动代理 /api /mcp /uploads 到 8097
cd frontend
npm install
npm run dev
```

访问 <http://localhost:5173>。

**构建生产前端**（输出到 `backend/crates/app/frontend-dist/`，供 `rust-embed` 内嵌）：

```bash
cd frontend && npm run build:check        # 类型检查 + 构建
cd ../backend && cargo build --release -p app   # 产物：target/release/mdocs
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
| `APP_MCP__ALLOWED_HOSTS` | 空 | MCP 端点额外允许的 Host（逗号分隔，容器 / 反向代理场景需要） |
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

暴露的工具：

| 工具 | 说明 |
| --- | --- |
| `list_projects` | 列出文档项目（默认仅公开项目） |
| `list_documents` | 列出某项目下的文档路径、标题与大小 |
| `read_document` | 读取指定文档的完整 Markdown 内容 |
| `search_documents` | 按关键词搜索并返回命中片段 |
| `get_link_graph` | 返回项目内文档间的有向链接图谱 |

- 默认只能访问**公开项目**；在「设置 → API Token」生成令牌后，通过 `Authorization: Bearer <token>` 可访问自己作为成员的项目。
- 客户端配置示例（以支持 Streamable HTTP 的 MCP 客户端为例）：

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

- 本地调试可使用 MCP Inspector：`npx @modelcontextprotocol/inspector`，连接 `http://localhost:8080/mcp`。

---

## 文档写作

项目支持两种链接写法，保存时会自动解析并更新链接树：

```markdown
[相对链接](./guide/install.md)
[带标题](../api/auth.md#登录)
[[wiki 链接]]
[[target-doc|显示名称]]
```

- 指向不存在的目标会生成「悬空链接」，在链接树中以红色虚线展示。
- 重命名文档时会同步更新引用该文档的链接。
- Markdown 渲染默认关闭内联 HTML（`html: false`）以防 XSS，后端不渲染 HTML。

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
│           │   └── services/      # vcs（版本控制）/ merge（三方合并）/ links（链接解析）/ import_export / avatar 等
│           └── frontend-dist/     # 前端构建产物（由 vite 生成，已 gitignore）
├── frontend/
│   ├── src/
│   │   ├── api/  stores/  router/  components/  utils/
│   │   └── views/                 # 首页 / 登录注册 / 项目工作台 / 公开站点 / 管理后台
│   └── vite.config.ts             # 开发代理 → 8097，构建输出 → backend/.../frontend-dist
├── docker-compose.yml
└── README.md
```

---

## 主要 REST 端点

除公开接口外均需 `Authorization: Bearer <JWT>`。

- **认证 / 用户**：`POST /api/auth/register`、`POST /api/auth/login`、`GET /api/auth/me`、`PATCH /api/me/profile`、`POST /api/me/avatar`
- **项目 / 成员**：`GET|POST /api/projects`、`GET|PATCH|DELETE /api/projects/{id}`、成员 CRUD
- **分支 / 文档**：`/api/projects/{id}/branches`、`tree`、`docs/*path`（保存携带 `base_commit_id` 乐观锁，冲突返回 409）
- **历史**：`docs/{docId}/history`、`versions/{versionId}`、`revert`、`commits/{id}`、`diff`
- **导入导出**：`import`、`export.zip`、`export.tree`
- **合并请求**：`mrs`、`merge-preview`、`reviews`、`merge`、`close`
- **链接树**：`branches/{branch}/link-tree`
- **公开**：`/api/public/...`（项目、文档、搜索、历史、链接）
- **健康检查**：`GET /api/health`

<br>

<p align="center"><sub>MDocs · Markdown 文档集分类管理系统</sub></p>
