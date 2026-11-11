//! `look://` (served as `http://look.localhost` on Windows): the file being previewed, by a
//! one-time token, with HTTP ranges so `<video>` can seek.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
    time::Instant,
};

use tauri::{
    AppHandle, Manager, UriSchemeResponder,
    http::{Request, Response, StatusCode},
};

use crate::app::M0;

/// Largest piece sent for an open-ended range.
const CHUNK: u64 = 8 << 20;

pub fn serve(app: &AppHandle, req: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let st = app.state::<Arc<M0>>().inner().clone();
    std::thread::spawn(move || responder.respond(respond(&st, &req)));
}

fn mime(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "wmv" => "video/x-ms-wmv",
        "ogv" => "video/ogg",
        "flv" => "video/x-flv",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "wma" => "audio/x-ms-wma",
        _ => "text/plain; charset=utf-8",
    }
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(code)
        .header("Access-Control-Allow-Origin", "*")
        .body(Vec::new())
        .unwrap()
}

/// `bytes=a-b` or `bytes=a-`.
fn range(req: &Request<Vec<u8>>) -> Option<(u64, Option<u64>)> {
    let v = req.headers().get("range")?.to_str().ok()?;
    let (a, b) = v.strip_prefix("bytes=")?.split_once('-')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()))
}

fn respond(st: &M0, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let t = Instant::now();
    let token = req
        .uri()
        .path()
        .strip_prefix("/f/")
        .and_then(|p| p.split('/').next())
        .unwrap_or_default()
        .to_owned();
    let path = st
        .shown
        .lock()
        .unwrap()
        .as_ref()
        .filter(|s| s.token == token)
        .map(|s| s.path.clone());
    let Some(path) = path else {
        return status(StatusCode::NOT_FOUND);
    };
    let Ok(mut f) = File::open(&path) else {
        return status(StatusCode::NOT_FOUND);
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let r = range(req);
    let (start, end, code) = match r {
        Some((s, _)) if s >= len && len > 0 => {
            return status(StatusCode::RANGE_NOT_SATISFIABLE);
        }
        Some((s, e)) => (
            s,
            e.unwrap_or(len.saturating_sub(1))
                .min(len.saturating_sub(1))
                .min(s + CHUNK - 1),
            StatusCode::PARTIAL_CONTENT,
        ),
        None => (0, len.saturating_sub(1), StatusCode::OK),
    };
    let n = if len == 0 { 0 } else { end - start + 1 };
    let mut body = vec![0u8; n as usize];
    if f.seek(SeekFrom::Start(start)).is_err() || f.read_exact(&mut body).is_err() {
        return status(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let mut b = Response::builder()
        .status(code)
        .header("Content-Type", mime(&path))
        .header("Accept-Ranges", "bytes")
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "no-store")
        .header("Content-Length", n.to_string());
    if code == StatusCode::PARTIAL_CONTENT {
        b = b.header("Content-Range", format!("bytes {start}-{end}/{len}"));
    }
    st.log(format!(
        "protocol {:?} → {} {} bytes in {} µs",
        r,
        code.as_u16(),
        n,
        t.elapsed().as_micros()
    ));
    b.body(body).unwrap()
}
