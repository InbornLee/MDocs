use std::io::Cursor;

use axum::body::Bytes;
use image::{imageops::FilterType, DynamicImage};
use uuid::Uuid;

use crate::{error::AppError, state::AppState};

/// Validate, downscale to 512x512 cover, encode as webp, store under avatars dir.
/// Returns the stored file name (e.g. "<uuid>.webp").
pub async fn save_avatar(state: &AppState, raw: Bytes) -> Result<String, AppError> {
    if raw.len() > state.cfg.max_avatar_mb * 1024 * 1024 {
        return Err(AppError::BadRequest(format!(
            "avatar too large (max {} MB)",
            state.cfg.max_avatar_mb
        )));
    }
    let img = image::load_from_memory(&raw)
        .map_err(|_| AppError::BadRequest("unsupported image format".into()))?;
    let img = resize_cover(img, 512);
    let mut webp = Vec::new();
    img.write_to(&mut Cursor::new(&mut webp), image::ImageFormat::WebP)
        .map_err(|e| AppError::Internal(format!("encode webp: {e}")))?;

    let dir = state.cfg.avatars_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| AppError::Internal(format!("create avatars dir: {e}")))?;
    let name = format!("{}.webp", Uuid::now_v7());
    tokio::fs::write(dir.join(&name), webp)
        .await
        .map_err(|e| AppError::Internal(format!("write avatar: {e}")))?;
    Ok(name)
}

fn resize_cover(img: DynamicImage, size: u32) -> DynamicImage {
    let (w, h) = (img.width(), img.height());
    let (nw, nh) = if w > h {
        let nh = (h as f32 * size as f32 / w as f32).round().max(1.0) as u32;
        (size, nh)
    } else {
        let nw = (w as f32 * size as f32 / h as f32).round().max(1.0) as u32;
        (nw, size)
    };
    let scaled = img.resize_exact(nw, nh, FilterType::Lanczos3);
    scaled.crop_imm(
        (nw.saturating_sub(size)) / 2,
        (nh.saturating_sub(size)) / 2,
        size.min(nw),
        size.min(nh),
    )
}
