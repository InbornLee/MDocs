use std::{env, path::PathBuf};

/// OpenAI 兼容的 embedding 服务（Ollama / vLLM / 云端均可）。
/// 未设置 `APP_EMBEDDING__URL` 时混合检索自动降级为纯 BM25。
#[derive(Clone, Debug)]
pub struct EmbeddingConfig {
    pub url: Option<String>,
    pub model: String,
    pub api_key: Option<String>,
    pub batch_size: usize,
    pub timeout_secs: u64,
}

#[derive(Clone, Debug)]
pub struct Config {
    /// e.g. "0.0.0.0:8080"
    pub listen: String,
    /// data directory (sqlite db, uploads)
    pub data_dir: PathBuf,
    /// sqlite://... or postgres://...
    pub database_url: String,
    pub jwt_secret: String,
    pub max_avatar_mb: usize,
    /// extra `Host` authorities accepted by the MCP endpoint
    pub mcp_allowed_hosts: Vec<String>,
    /// read_document 单次返回的字节上限（超出截断并标记 truncated）
    pub mcp_max_content_bytes: usize,
    /// /mcp 每分钟请求上限；0 表示关闭
    pub mcp_rate_limit_per_min: u32,
    /// 分支快照缓存保留的 head 数量
    pub snapshot_cache_size: usize,
    /// 对外站点基地址（用于 MCP 返回绝对链接），如 https://docs.example.com
    pub public_base_url: Option<String>,
    pub embedding: EmbeddingConfig,
}

impl Config {
    pub fn from_env() -> Self {
        let data_dir =
            PathBuf::from(env::var("APP_DATA_DIR").unwrap_or_else(|_| "./data".to_string()));
        let database_url = env::var("APP_DATABASE__URL")
            .unwrap_or_else(|_| format!("sqlite://{}/app.db?mode=rwc", data_dir.display()));
        let jwt_secret = env::var("APP_JWT__SECRET").unwrap_or_else(|_| {
            use rand::RngCore;
            let mut buf = [0u8; 32];
            rand::rng().fill_bytes(&mut buf);
            tracing::warn!("APP_JWT__SECRET not set, using ephemeral random secret (sessions invalidated on restart)");
            buf.iter().map(|b| format!("{b:02x}")).collect()
        });
        let mcp_allowed_hosts = env::var("APP_MCP__ALLOWED_HOSTS")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|h| h.trim().to_string())
                    .filter(|h| !h.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        Self {
            listen: env::var("APP_LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            database_url,
            jwt_secret,
            max_avatar_mb: env::var("APP_UPLOAD__MAX_AVATAR_MB")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
            data_dir,
            mcp_allowed_hosts,
            mcp_max_content_bytes: env_usize("APP_MCP__MAX_CONTENT_BYTES", 64 * 1024),
            mcp_rate_limit_per_min: env_usize("APP_MCP__RATE_LIMIT_PER_MIN", 240) as u32,
            snapshot_cache_size: env_usize("APP_MCP__SNAPSHOT_CACHE", 8),
            public_base_url: env::var("APP_PUBLIC_BASE_URL")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            embedding: EmbeddingConfig {
                url: env::var("APP_EMBEDDING__URL")
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                model: env::var("APP_EMBEDDING__MODEL").unwrap_or_else(|_| "bge-m3".to_string()),
                api_key: env::var("APP_EMBEDDING__API_KEY")
                    .ok()
                    .filter(|s| !s.trim().is_empty()),
                batch_size: env_usize("APP_EMBEDDING__BATCH_SIZE", 16).clamp(1, 128),
                timeout_secs: env_usize("APP_EMBEDDING__TIMEOUT_SECS", 30).max(1) as u64,
            },
        }
    }

    /// Host authorities the MCP endpoint accepts. Streamable HTTP defaults to
    /// loopback-only (DNS-rebinding protection); containers and reverse proxies
    /// need their own hostname, so merge the configured listen address plus any
    /// explicit `APP_MCP__ALLOWED_HOSTS` entries.
    pub fn mcp_allowed_hosts(&self) -> Vec<String> {
        let mut hosts = vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
            "0.0.0.0".to_string(),
            "[::1]".to_string(),
        ];
        let listen = &self.listen;
        hosts.push(listen.clone());
        let bare = listen.rsplit_once(':').map(|(h, _)| h).unwrap_or(listen);
        if !bare.is_empty() {
            hosts.push(bare.to_string());
            hosts.push(format!("{}:80", bare));
            hosts.push(format!("{}:8080", bare));
        }
        hosts.extend(self.mcp_allowed_hosts.iter().cloned());
        hosts.sort();
        hosts.dedup();
        hosts
    }

    pub fn uploads_dir(&self) -> PathBuf {
        self.data_dir.join("uploads")
    }

    pub fn avatars_dir(&self) -> PathBuf {
        self.uploads_dir().join("avatars")
    }

    /// Project attachments (images and other non-markdown files imported with
    /// the documents), one sub-directory per project id.
    pub fn attachments_dir(&self) -> PathBuf {
        self.uploads_dir().join("attachments")
    }
}

fn env_usize(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}
