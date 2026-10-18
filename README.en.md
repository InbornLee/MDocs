# MDocs

> **Single-binary, AI-native Markdown knowledge base** — version docs like Git, search like a search engine.

[中文](README.md) · [English](README.en.md)

MDocs turns scattered Markdown into a collaborative, traceable knowledge base that AI can use directly:

- **Lightweight**: one Rust binary serves the SPA, REST API and MCP endpoint; SQLite by default, no Node / Python / JVM runtime.
- **AI-native**: built-in MCP Streamable HTTP with 11 tools, resources and a citation-oriented prompt. Every hit carries a URL, anchor, line range and version.
- **Retrieval-first**: Chinese bigram + BM25 inverted index, optional vector hybrid ranking (RRF), heading-aware chunks, and section-level reading instead of dumping whole documents into a model.

Current version: **0.1.1**. Evaluation, roadmap and trade-offs are in [docs/AI-KNOWLEDGE-BASE.md](docs/AI-KNOWLEDGE-BASE.md).

---

## Why MDocs

| | |
| --- | --- |
| **Single binary** | The SPA is embedded with `rust-embed`; static files, REST API and MCP are served by one process. No separate frontend server, reverse proxy or worker. |
| **Zero external dependencies at startup** | SQLite + local file storage by default — `docker run` is enough. PostgreSQL and embeddings are optional. |
| **Small image** | Debian slim runtime, about 46 MB compressed; no Node, Python or JVM. |
| **AI-ready** | No gateway or plugin required: an AI client connects to `/mcp` and can search, read sections, inspect history and list attachments. |
| **Explainable retrieval** | Every hit traces back to a path, heading, anchor, line range and version — not just a similar blob of text. |
| **China-friendly builds** | Docker builds default to mainland-China apt / npm / crates.io mirrors. |

---

## Core capabilities

### Documents and collaboration

- Tree browsing, online editing (CodeMirror 6), attachments and images, full-text search
- Branches, commit history, arbitrary version restore, revert, two-version diff
- Line-level three-way merge; branch merges go through merge requests with preview, review and conflict resolution
- Project members and roles (owner / editor / reader)
- Public site: projects and individual documents can be published separately

### Metadata and links

- Parses front-matter `tags` / `description`, shows them in the reader and supports `tag:` filtering
- Resolves relative links `[text](./path/doc.md)` and wiki links `[[wiki]]` / `[[name|label]]`
- ECharts force-directed link graph; dangling links shown as red dashed lines; renames update references

### Import / export

- Import `.zip` vaults, single `.md` files or tree JSON
- Export zip / tree JSON / single document; handles Chinese and GBK file names in archives

### Deployment

- SQLite by default; PostgreSQL optional
- Single-container deployment; migrations run at startup
- Mainland-China mirrors by default for builds

---

## How retrieval works

MDocs retrieval is not `LIKE` over whole documents. It is a four-layer design built for AI use:

### 1. Tokenization: Chinese without extensions

- Chinese uses **bigrams** (`人民医院` → 人民 / 民医 / 医院), with single-character fallback
- English and digits are tokenized as words, case-folded
- No SQLite FTS5, PostgreSQL `pg_trgm` or Chinese tokenizer required; SQLite and PostgreSQL behave identically

### 2. Indexing: heading-aware chunks, per-version

- Documents are split by heading; code blocks and tables stay intact; oversized sections are split by lines/bytes
- `doc_chunks` stores text, heading, anchor and line range; `chunk_terms` stores the `(term, tf)` inverted index
- Indexes are built per `doc_versions` (immutable); search filters by the head versions of the requested branch
- Index writes happen in the **same transaction** as version creation; history is backfilled in the background; missing versions are indexed lazily on search

### 3. Recall and ranking: explainable BM25

- The inverted index coarse-ranks chunks by `SUM(tf)`
- Application-level BM25 reranking (k1=1.2, b=0.75), with title/path hit boosts
- Results are deduplicated by `(document, section)` and return `score`, `section`, `anchor`, `line_start` / `line_end`

### 4. Hybrid retrieval: BM25 + vectors, with graceful fallback

- Optional OpenAI-compatible embeddings (Ollama / vLLM / cloud)
- `chunk_vectors` stores f32 BLOBs; cosine similarity runs in the application — no sqlite-vec or pgvector required
- BM25 top50 + vector top50 fused with **RRF (k=60)**; if embeddings are unset or fail, retrieval falls back to BM25
- A background worker fills missing vectors every 30 seconds and rebuilds them when the model changes

### 5. Context: let the model read the right section

- `get_document_outline` returns the heading tree and anchors
- `read_document(section=...)` returns only that section; `offset` + `limit` read by line
- Responses include `total_lines` / `total_bytes` / `truncated`, so context does not silently overflow
- Search hits carry `url#anchor`, so answers can cite a clickable source

### 6. Scale and measurements

- Sample dataset: 416 historical versions, 3,293 chunks, 321k inverted-index rows
- History backfill takes about 15 seconds; a search takes roughly **10–90 ms** at this scale
- Vector search scans in the application, suitable for thousands to tens of thousands of chunks; larger corpora can use SQLite-vec / pgvector ANN

---

## Quick start

### Docker Compose (recommended)

The repository `docker-compose.yml` connects to an **external PostgreSQL on the host** by default and mounts the `mdocs-data` volume for uploads.

1. Prepare a database (choose one):

   ```bash
   # A. External PostgreSQL (compose default; create the database first)
   #    URL: postgresql://mdoc:mdoc@host.docker.internal:5432/mdoc
   createdb mdoc

   # B. Built-in SQLite: edit docker-compose.yml and set APP_DATABASE__URL to
   #    sqlite:///data/app.db?mode=rwc
   ```

2. Set a secret and start:

   ```bash
   cp .env.example .env
   # edit .env and at least set APP_JWT__SECRET
   # generate one with: openssl rand -hex 32
   docker compose up -d --build
   ```

3. Open <http://localhost:8008> (change the host port with `MDOCS_PORT`).

> Migrations run automatically at startup; no manual schema setup is needed.

### Local development

Requirements: Rust 1.97+, Node.js 20.19+ (the Docker image uses Node 24).

```bash
# 1) Backend on 8097 (matches the Vite proxy)
cd backend
APP_LISTEN=0.0.0.0:8097 APP_JWT__SECRET=dev-secret cargo run -p app
# SQLite by default: ./backend/data/app.db

# 2) Frontend dev server on 5173, proxying /api /mcp /uploads to 8097
cd frontend
npm install
npm run dev
```

Open <http://localhost:5173>.

**Production frontend build** (outputs to `backend/crates/app/frontend-dist/` for `rust-embed`):

```bash
cd frontend && npm run build:check
cd ../backend && cargo build --release -p app
# binary: target/release/mdocs
```

---

## Configuration

Backend environment variables (`backend/crates/app/src/config.rs`):

| Variable | Default | Description |
| --- | --- | --- |
| `APP_LISTEN` | `0.0.0.0:8080` | Listen address |
| `APP_DATA_DIR` | `./data` | Data directory (SQLite database and uploads) |
| `APP_DATABASE__URL` | `sqlite://<data_dir>/app.db?mode=rwc` | Database URL; `sqlite://` or `postgres://` |
| `APP_JWT__SECRET` | random temporary value | JWT signing key; sessions are invalidated on restart if unset |
| `APP_UPLOAD__MAX_AVATAR_MB` | `5` | Avatar upload limit (MB) |
| `APP_PUBLIC_BASE_URL` | empty | Public site URL; makes MCP links absolute |
| `APP_MCP__ALLOWED_HOSTS` | empty | Extra Host authorities accepted by `/mcp` (comma separated) |
| `APP_MCP__MAX_CONTENT_BYTES` | `65536` | Max bytes returned by `read_document` per call |
| `APP_MCP__RATE_LIMIT_PER_MIN` | `240` | `/mcp` requests per minute per token/IP; `0` disables |
| `APP_MCP__SNAPSHOT_CACHE` | `8` | Branch snapshot cache size (number of heads) |
| `APP_EMBEDDING__URL` | empty | OpenAI-compatible embeddings endpoint (e.g. Ollama `/v1/embeddings`); empty = BM25 only |
| `APP_EMBEDDING__MODEL` | `bge-m3` | Embedding model name |
| `APP_EMBEDDING__API_KEY` | empty | Bearer token for the embedding service |
| `APP_EMBEDDING__BATCH_SIZE` | `16` | Chunks per embedding batch |
| `APP_EMBEDDING__TIMEOUT_SECS` | `30` | Embedding request timeout |
| `RUST_LOG` | `info` | Log level |

Docker build arguments (defaults point to mainland-China mirrors; pass an empty value to use upstream):

| Argument | Default |
| --- | --- |
| `APT_MIRROR` | `http://mirrors.aliyun.com` |
| `NPM_REGISTRY` | `https://registry.npmmirror.com` |
| `CARGO_REGISTRY_MIRROR` | `sparse+https://rsproxy.cn/index/` |

---

## AI integration (MCP)

The MCP endpoint shares the HTTP port with the REST API: `/mcp` (Streamable HTTP).

### Tools

| Tool | Description |
| --- | --- |
| `list_projects` | List projects (public only by default), with default branch and document count |
| `list_branches` | List branches |
| `list_documents` | Paths, titles, sizes, tags and reading URLs |
| `list_tags` | Front-matter tags with document counts |
| `get_document_outline` | Heading tree (level / title / anchor / line) |
| `read_document` | Read full text or a specific `version_id`; supports `section` / `offset`+`limit`; returns `is_current` / `truncated` |
| `search_documents` | BM25 + optional vector hybrid search; returns `score`, `section`, `anchor`, `url`; supports cross-project search, pagination and `tag` filters |
| `get_link_graph` | Directed link graph between documents |
| `get_history` | Version history (commit, author, version id, current flag) |
| `get_diff` | Unified diff between two versions |
| `list_attachments` | Attachments referenced by a document (path / URL / MIME / size); `all=true` lists every project attachment |

### Resources and prompt

- **Resources**: `mdocs://{project}/{path}` plus a resource template, so clients can fetch documents by URI
- **Prompt**: `answer_with_citations` — search first, read sections, then answer with links and anchors

### Access and governance

- Anonymous callers only see **public projects** and published documents
- Create an API token in Settings → API Token (with optional expiry) and send `Authorization: Bearer <token>` to reach projects you are a member of
- Per-token/IP rate limiting; 429 responses include `Retry-After` and `RateLimit-*`
- Every tool call is written to `mcp_audit_log`: tool, project, argument summary, caller, IP, success/failure

### Client configuration

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

For local debugging, use MCP Inspector: `npx @modelcontextprotocol/inspector` and connect to `http://localhost:8080/mcp`.

---

## Writing documents

Two link styles are supported; saving updates the link graph automatically:

```markdown
[Relative link](./guide/install.md)
[With anchor](../api/auth.md#login)
[[wiki link]]
[[target-doc|display name]]
```

Front-matter tags and description go into the metadata index but do not pollute body retrieval:

```markdown
---
tags: [his, inpatient]
description: Core hospital business system documentation
---
```

- Links to missing targets become dangling links (red dashed edges in the graph)
- Renaming a document updates references
- Inline HTML is disabled (`html: false`) to prevent XSS; the backend never renders HTML

---

## Tech stack

| Layer | Technology |
| --- | --- |
| Backend | Rust · axum 0.8 · tokio · SeaORM 1.1 (SQLite / PostgreSQL) · rmcp 3.3 (MCP) · comrak (Markdown AST) · similar (diff / three-way merge) · tower-http (compression / rate limiting) · jsonwebtoken · argon2 |
| Retrieval | Custom bigram/word tokenization · inverted index + BM25 · optional OpenAI-compatible embeddings + RRF fusion |
| Frontend | Vue 3.5 · Vite 8 · TypeScript · Naive UI · Pinia · Vue Router · CodeMirror 6 · markdown-it · highlight.js · ECharts · Mermaid |
| Deployment | Three-stage Docker build (Node builds the SPA → Rust embeds it → Debian slim runtime); one container serves SPA + REST API + MCP |

---

## Project layout

```
mdocs-main/
├── backend/
│   ├── Dockerfile                 # three-stage build: frontend → backend → runtime
│   ├── Cargo.toml                 # workspace
│   └── crates/
│       ├── migration/             # SeaORM migrations (all tables)
│       └── app/
│           ├── src/
│           │   ├── main.rs        # config → DB → migrations → Router (API + /mcp + /uploads + SPA fallback)
│           │   ├── config.rs  state.rs  error.rs  mcp.rs
│           │   ├── entities/      # SeaORM entities
│           │   ├── routes/        # auth user project document member mr transfer public admin
│           │   └── services/      # vcs / merge / links / index / search_index / metadata / embedding / snapshot / audit / rate_limit
│           └── frontend-dist/     # SPA build output (generated by vite, gitignored)
├── frontend/
│   ├── src/
│   │   ├── api/  stores/  router/  components/  utils/  content/
│   │   └── views/                 # home / login / project workspace / public site / admin
│   └── vite.config.ts             # dev proxy → 8097, build output → backend/.../frontend-dist
├── docker-compose.yml
├── docs/                          # evaluation and roadmap (AI-KNOWLEDGE-BASE.md)
└── README.md
```

---

## Main REST endpoints

All non-public endpoints require `Authorization: Bearer <JWT>`.

- **Auth / users**: `POST /api/auth/register`, `POST /api/auth/login`, `GET /api/auth/me`, `PATCH /api/me/profile`, `POST /api/me/avatar`
- **Projects / members**: `GET|POST /api/projects`, `GET|PATCH|DELETE /api/projects/{slug}`, member CRUD
- **Branches / documents**: `/api/projects/{slug}/branches`, `tree`, `docs` (saves carry `base_version_id` for optimistic locking, 409 on conflict), `tags`
- **History**: `docs/{doc_id}/history`, `versions/{version_id}`, `revert`, `commits/{id}`, `diff`
- **Import / export**: `import`, `export.zip`, `export.tree`, `docs/{doc_id}/export`
- **Merge requests**: `mrs`, `merge-preview`, `reviews`, `merge`, `close`
- **Link graph**: `branches/{branch}/link-tree`
- **Attachments**: `/uploads/attachments/{project_id}/{path}` (public projects are anonymous-readable; private projects require a member JWT / API token)
- **Public**: `/api/public/...` (projects, documents, search, history, tags, links)
- **Health / version**: `GET /api/health`, `GET /api/version`

---

## Roadmap

- **0.1.0**: first release (documents, version control, MRs, link graph, public site, basic MCP)
- **0.1.1**: AI knowledge-base P0 + P1
  - chunk inverted index + BM25
  - optional vector hybrid retrieval (RRF)
  - front-matter tags / description
  - version reading, history, diff, attachment listing
  - MCP audit, rate limiting, token expiry, attachment access control
- **Next**: OAuth 2.1 / token scopes, signed attachment URLs, ANN index, built-in `ask` and knowledge-quality checks

See [docs/AI-KNOWLEDGE-BASE.md](docs/AI-KNOWLEDGE-BASE.md) for details.

## License

[MIT](LICENSE) © 2026 Inborn Lee (李颖博)

<br>

<p align="center"><sub>MDocs · single-binary, AI-native Markdown knowledge base</sub></p>
