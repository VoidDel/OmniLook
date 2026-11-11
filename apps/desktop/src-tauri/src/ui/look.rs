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
    let uri = req.uri().path();
    let path = if let Some(rest) = uri.strip_prefix("/f/") {
        rest.split('/')
            .next()
            .and_then(|token| state.path_for(token))
    } else if let Some(rest) = uri.strip_prefix("/r/") {
        // An image a Markdown file shows: `/r/<token>/<its address, encoded>`.
        rest.split_once('/').and_then(|(token, dest)| {
            let md = state.path_for(token)?;
            local_image(&md, &decode(dest))
        })
    } else {
        None
    };
    std::thread::spawn(move || {
        responder.respond(match path {
            Some(p) => respond(&p, &req),
            None => status(StatusCode::NOT_FOUND),
        })
    });
}

/// Percent-encodes everything but the unreserved characters (an address inside a URL path).
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Undoes percent-encoding; a bad escape is kept as it is.
pub fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

const IMAGES: &[&str] = &[
    "png", "jpg", "jpeg", "jfif", "gif", "webp", "bmp", "ico", "svg", "avif", "tif", "tiff",
];

/// The image file that `dest` (as written in the Markdown file `md`) names: relative to the
/// file's folder, or a full path, or a `file:` URL; its own `%20`-style escapes undone when that
/// names a file. Only images: a Markdown file cannot make the window read anything else.
pub fn local_image(md: &Path, dest: &str) -> Option<std::path::PathBuf> {
    let dest = dest.split(['?', '#']).next().unwrap_or_default();
    let dest = dest
        .strip_prefix("file:///")
        .or_else(|| dest.strip_prefix("file://"))
        .unwrap_or(dest);
    let base = md.parent()?;
    let candidates = [decode(dest), dest.to_owned()];
    candidates.iter().find_map(|d| {
        let p = Path::new(d.trim_start_matches(['/', '\\']));
        let p = if Path::new(d).is_absolute() {
            Path::new(d).to_path_buf()
        } else {
            base.join(p)
        };
        let ext = p
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        (IMAGES.contains(&ext.as_str()) && p.is_file()).then_some(p)
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_round_trip_and_only_images_are_served() {
        let s = "img/a b%20c.png?raw=1#x";
        assert_eq!(decode(&encode(s)), s);
        let d = std::env::temp_dir().join(format!("omnilook-look-{}", std::process::id()));
        std::fs::create_dir_all(d.join("img")).unwrap();
        let md = d.join("README.md");
        std::fs::write(&md, "# x").unwrap();
        std::fs::write(d.join("img").join("a b.png"), b"png").unwrap();
        std::fs::write(d.join("secret.txt"), b"no").unwrap();
        let want = d.join("img").join("a b.png");
        assert_eq!(local_image(&md, "img/a%20b.png"), Some(want.clone()));
        assert_eq!(local_image(&md, "img/a b.png?raw=true"), Some(want.clone()));
        assert_eq!(local_image(&md, "./img/a b.png"), Some(want));
        assert_eq!(local_image(&md, "secret.txt"), None);
        assert_eq!(local_image(&md, "missing.png"), None);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
