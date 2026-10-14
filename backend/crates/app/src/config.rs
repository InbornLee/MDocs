use std::{env, path::PathBuf};

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
