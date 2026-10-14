//! Project attachments: images and any other non-markdown file that arrives
//! with an import.
//!
//! They are stored on disk under `data_dir/uploads/attachments/<project id>/`
//! mirroring the path inside the project, so a markdown reference like
//! `assets/x.png` maps back to the same relative path when the project is
//! exported again.

use std::path::PathBuf;

use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    state::AppState,
};

/// Largest single attachment accepted on import (32 MB).
pub const MAX_ASSET_BYTES: usize = 32 * 1024 * 1024;

/// Validate a project-relative attachment path.
///
/// Same shape as `vcs::validate_doc_path` minus the markdown extension: the
/// path is what ends up joined onto the attachment directory, so every escape
/// (`..`, absolute paths, Windows separators) has to be rejected here.
pub fn validate_asset_path(path: &str) -> AppResult<()> {
    let bad = || AppError::BadRequest(format!("invalid asset path '{path}'"));
    if path.is_empty() || path.len() > 512 {
        return Err(bad());
    }
    if path.starts_with('/') || path.ends_with('/') {
        return Err(bad());
    }
    if path.contains('\\') || path.contains('\0') || path.contains(':') {
        return Err(bad());
    }
    if path.split('/').any(|seg| seg.is_empty() || seg == "." || seg == "..") {
        return Err(bad());
    }
    Ok(())
}

/// Directory holding one project's attachments.
pub fn project_dir(state: &AppState, project_id: Uuid) -> PathBuf {
    state.cfg.attachments_dir().join(project_id.to_string())
}

/// Persist one attachment, replacing a previous file at the same path.
pub async fn store(
    state: &AppState,
    project_id: Uuid,
    rel_path: &str,
    bytes: &[u8],
) -> AppResult<()> {
    validate_asset_path(rel_path)?;
    if bytes.len() > MAX_ASSET_BYTES {
        return Err(AppError::BadRequest(format!(
            "attachment larger than {}MB",
            MAX_ASSET_BYTES / 1024 / 1024
        )));
    }
    let dest = project_dir(state, project_id).join(rel_path);
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::Internal(format!("create attachments dir: {e}")))?;
    }
    tokio::fs::write(&dest, bytes)
        .await
        .map_err(|e| AppError::Internal(format!("write attachment: {e}")))?;
    Ok(())
}

/// Read an attachment back.
pub async fn read(state: &AppState, project_id: Uuid, rel_path: &str) -> AppResult<Vec<u8>> {
    validate_asset_path(rel_path)?;
    let dest = project_dir(state, project_id).join(rel_path);
    tokio::fs::read(&dest)
        .await
        .map_err(|_| AppError::NotFound("attachment not found".into()))
}

/// Every stored attachment of a project, as project-relative paths.
pub async fn list_all(state: &AppState, project_id: Uuid) -> AppResult<Vec<String>> {
    let root = project_dir(state, project_id);
    let mut out: Vec<String> = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(e) => e,
            // a project without attachments simply has no directory yet
            Err(_) => continue,
        };
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| AppError::Internal(format!("read attachments dir: {e}")))?
        {
            let path = entry.path();
            let is_dir = entry
                .file_type()
                .await
                .map_err(|e| AppError::Internal(format!("read attachments dir: {e}")))?
                .is_dir();
            if is_dir {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(&root) {
                // stored paths use '/' on every platform
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    Ok(out)
}
