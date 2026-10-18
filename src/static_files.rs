//! Serves the single-page app shell, PWA manifest, service worker, and icons
//! from the project root. The server intentionally refuses to serve anything
//! under `data/` so inventory and auth files are never exposed over HTTP.

use std::path::{Path, PathBuf};

use axum::{
    body::Body,
    extract::Path as AxumPath,
    http::{header, HeaderValue, Response, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};

pub fn attach(api: Router) -> Router {
    api.route("/", get(serve_index))
        .route("/index.html", get(serve_index))
        .route("/manifest.webmanifest", get(serve_manifest))
        .route("/service-worker.js", get(serve_service_worker))
        .route("/icons/:name", get(serve_icon))
}

fn asset_root() -> PathBuf {
    std::env::var("STOCKSPACE_ASSET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

async fn serve_index() -> impl IntoResponse {
    read_asset(&asset_root().join("index.html"), "text/html; charset=utf-8")
}

async fn serve_manifest() -> impl IntoResponse {
    read_asset(
        &asset_root().join("manifest.webmanifest"),
        "application/manifest+json",
    )
}

async fn serve_service_worker() -> impl IntoResponse {
    // Scope the service worker to "/" by advertising the header browsers check.
    match read_file(&asset_root().join("service-worker.js")) {
        Ok(bytes) => {
            let mut resp = Response::new(Body::from(bytes));
            resp.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/javascript; charset=utf-8"),
            );
            resp.headers_mut()
                .insert("service-worker-allowed", HeaderValue::from_static("/"));
            resp.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
            resp
        }
        Err(resp) => resp,
    }
}

async fn serve_icon(AxumPath(name): AxumPath<String>) -> impl IntoResponse {
    // Reject any path traversal: only a bare filename, no separators.
    if name.contains('/') || name.contains('\\') || name.contains("..") || name.is_empty() {
        return not_found();
    }
    let path = asset_root().join("icons").join(&name);
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    read_asset(&path, mime.essence_str())
}

fn read_asset(path: &Path, content_type: &str) -> Response<Body> {
    match read_file(path) {
        Ok(bytes) => {
            let mut resp = Response::new(Body::from(bytes));
            resp.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(content_type)
                    .unwrap_or(HeaderValue::from_static("application/octet-stream")),
            );
            resp.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
            resp
        }
        Err(resp) => resp,
    }
}

fn read_file(path: &Path) -> Result<Vec<u8>, Response<Body>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(_) => Err(not_found()),
    }
}

fn not_found() -> Response<Body> {
    let mut resp = Response::new(Body::from("not found"));
    *resp.status_mut() = StatusCode::NOT_FOUND;
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    resp
}
