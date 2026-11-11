//! `look://` (`http://look.localhost` on Windows, `docs/development-plan.md` §8): the file being
//! previewed, reachable only through its one-time token, with HTTP ranges so media can seek.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use tauri::{
    AppHandle, Manager, UriSchemeResponder,
    http::{Request, Response, StatusCode},
};

use super::ViewState;

/// Largest piece sent for one range request.
const CHUNK: u64 = 8 << 20;
/// Largest file sent whole when no range is asked for (images, PDF).
const WHOLE: u64 = 512 << 20;

pub fn serve(app: &AppHandle, req: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let state = app.state::<ViewState>();
    let path = req
        .uri()
        .path()
        .strip_prefix("/f/")
        .and_then(|rest| rest.split('/').next())
        .and_then(|token| state.path_for(token));
    std::thread::spawn(move || {
        responder.respond(match path {
            Some(p) => respond(&p, &req),
            None => status(StatusCode::NOT_FOUND),
        })
    });
}

pub fn mime(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" | "jfif" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        "ogv" => "video/ogg",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        _ => "application/octet-stream",
    }
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(code)
        .body(Vec::new())
        .expect("response")
}

/// `bytes=a-b` or `bytes=a-`.
fn range(req: &Request<Vec<u8>>) -> Option<(u64, Option<u64>)> {
    let v = req.headers().get("range")?.to_str().ok()?;
    let (a, b) = v.strip_prefix("bytes=")?.split_once('-')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()))
}

fn respond(path: &Path, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    // Images the WebView cannot decode come as PNG.
    if omnilook_core::image::needs_conversion(path) {
        return match omnilook_core::image::to_png(path) {
            Ok(png) => Response::builder()
                .header("Content-Type", "image/png")
                .header("Cache-Control", "no-store")
                .body(png)
                .expect("response"),
            Err(_) => status(StatusCode::UNSUPPORTED_MEDIA_TYPE),
        };
    }
    let Ok(mut f) = File::open(path) else {
        return status(StatusCode::NOT_FOUND);
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let (start, end, code) = match range(req) {
        Some((s, _)) if s >= len && len > 0 => return status(StatusCode::RANGE_NOT_SATISFIABLE),
        Some((s, e)) => (
            s,
            e.unwrap_or(u64::MAX)
                .min(len.saturating_sub(1))
                .min(s + CHUNK - 1),
            StatusCode::PARTIAL_CONTENT,
        ),
        None if len > WHOLE => return status(StatusCode::PAYLOAD_TOO_LARGE),
        None => (0, len.saturating_sub(1), StatusCode::OK),
    };
    let n = if len == 0 { 0 } else { end - start + 1 };
    let mut body = vec![0u8; n as usize];
    if f.seek(SeekFrom::Start(start)).is_err() || f.read_exact(&mut body).is_err() {
        return status(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let mut b = Response::builder()
        .status(code)
        .header("Content-Type", mime(path))
        .header("Accept-Ranges", "bytes")
        .header("Access-Control-Allow-Origin", "http://tauri.localhost")
        .header("Cache-Control", "no-store")
        .header("Content-Length", n.to_string());
    if code == StatusCode::PARTIAL_CONTENT {
        b = b.header("Content-Range", format!("bytes {start}-{end}/{len}"));
    }
    b.body(body).expect("response")
}
