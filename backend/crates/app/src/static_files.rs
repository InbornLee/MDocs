use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "frontend-dist/"]
struct Assets;

fn asset_response(path: &str) -> Response {
    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let mut resp = (
                [(header::CONTENT_TYPE, mime.as_ref().to_string())],
                content.data,
            )
                .into_response();
            if path == "index.html" {
                resp.headers_mut().insert(
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("no-cache"),
                );
            }
            resp
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

/// SPA fallback: serve an embedded asset when the path matches one, otherwise
/// hand back index.html so client-side routes work on a hard load or refresh.
///
/// Client routes legitimately contain dots — document paths end in `.md` — so
/// the fallback must not treat "has an extension" as "missing static file".
/// Only the bundled asset directory is a real 404 when absent.
pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if Assets::get(path).is_some() {
        return asset_response(path);
    }
    if path.starts_with("assets/") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    asset_response("index.html")
}
