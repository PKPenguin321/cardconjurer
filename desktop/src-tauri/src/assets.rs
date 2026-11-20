//! Serves the CardConjurer files (HTML, JS, CSS, fonts and the ~2 GB of frame
//! images) straight from disk through the app's own `cc://` URL scheme.
//!
//! The creator's code uses root-relative paths everywhere (`/img/...`,
//! `/js/...`, `/fonts/...`), so the repo folder is served as the site root.
//! Nothing is embedded in the executable and nothing touches the network.
//!
//! Like `cards.rs`, this is standard-library only so it can be unit-tested
//! without Tauri.

use std::fs;
use std::path::{Component, Path, PathBuf};

/// Environment variable that overrides where the assets are loaded from.
pub const ASSETS_ENV_VAR: &str = "CARDCONJURER_ASSETS";

/// The page shell; `/` is this file with the creator's HTML inserted.
const SHELL_PATH: &str = "desktop/web/shell.html";
const CREATOR_PATH: &str = "creator/index.html";
const CREATOR_MARKER: &str = "<!-- CREATOR_CONTENT -->";

pub struct Asset {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

/// Finds the folder holding the CardConjurer files. Checks, in order:
/// 1. the `CARDCONJURER_ASSETS` environment variable,
/// 2. the folder the executable is in, its parents, and an `assets` folder
///    next to any of them (covers an installed copy and `target/release`),
/// 3. the repo this crate was built from (covers `cargo run`).
pub fn find_root(exe: Option<&Path>, build_repo: &Path) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(ASSETS_ENV_VAR) {
        let dir = PathBuf::from(dir);
        if is_root(&dir) {
            return Some(dir);
        }
        eprintln!("{ASSETS_ENV_VAR} is set but {} doesn't contain CardConjurer", dir.display());
    }
    if let Some(exe) = exe {
        for dir in exe.ancestors().skip(1) {
            for candidate in [dir.join("assets"), dir.to_path_buf()] {
                if is_root(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }
    is_root(build_repo).then(|| build_repo.to_path_buf())
}

fn is_root(dir: &Path) -> bool {
    dir.join(SHELL_PATH).is_file() && dir.join(CREATOR_PATH).is_file() && dir.join("img").is_dir()
}

/// Answers a request for `uri_path` (the path part of the URL).
pub fn serve(root: Option<&Path>, uri_path: &str) -> Asset {
    let Some(root) = root else {
        return html(
            500,
            format!(
                "<h1>CardConjurer files not found</h1><p>Put the CardConjurer folder in an \
                 <code>assets</code> folder next to the app, or set the <code>{ASSETS_ENV_VAR}</code> \
                 environment variable to its location.</p>"
            ),
        );
    };
    if matches!(uri_path, "" | "/" | "/index.html") {
        return creator_page(root);
    }
    let Some(path) = resolve(root, uri_path) else {
        return not_found();
    };
    match fs::read(&path) {
        Ok(body) => Asset { status: 200, content_type: content_type(&path), body },
        Err(_) => not_found(),
    }
}

/// The shell page with the creator's HTML dropped in, so the creator runs as
/// a normal page (no htmx, no menu, no other tools) and upstream edits to
/// `creator/index.html` carry over automatically.
fn creator_page(root: &Path) -> Asset {
    let shell = fs::read_to_string(root.join(SHELL_PATH));
    let creator = fs::read_to_string(root.join(CREATOR_PATH));
    match (shell, creator) {
        (Ok(shell), Ok(creator)) if shell.contains(CREATOR_MARKER) => {
            html(200, shell.replacen(CREATOR_MARKER, &creator, 1))
        }
        _ => html(500, format!("<h1>Couldn't build the creator page</h1><p>Check {SHELL_PATH} and {CREATOR_PATH}.</p>")),
    }
}

/// Maps a URL path to a file under `root`, refusing anything that would
/// escape it.
pub fn resolve(root: &Path, uri_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(uri_path.split(['?', '#']).next()?)?;
    let mut path = root.to_path_buf();
    for segment in decoded.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        // Each URL segment must be exactly one plain path component.
        let mut components = Path::new(segment).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) if !segment.contains(['\\', ':']) => path.push(name),
            _ => return None,
        }
    }
    path.is_file().then_some(path)
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

pub fn content_type(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "webmanifest" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn html(status: u16, body: String) -> Asset {
    Asset { status, content_type: "text/html; charset=utf-8", body: body.into_bytes() }
}

fn not_found() -> Asset {
    Asset { status: 404, content_type: "text/plain; charset=utf-8", body: b"Not found".to_vec() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("cc-assets-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("desktop/web")).unwrap();
        fs::create_dir_all(root.join("creator")).unwrap();
        fs::create_dir_all(root.join("img/frames")).unwrap();
        fs::write(root.join(SHELL_PATH), format!("<body>{CREATOR_MARKER}</body>")).unwrap();
        fs::write(root.join(CREATOR_PATH), "<div id='creator'></div>").unwrap();
        fs::write(root.join("img/frames/a b.png"), [137, 80, 78, 71]).unwrap();
        root
    }

    #[test]
    fn serves_files_and_creator_page() {
        let root = fake_root("serve");
        let page = serve(Some(&root), "/");
        assert_eq!(page.status, 200);
        assert_eq!(String::from_utf8(page.body).unwrap(), "<body><div id='creator'></div></body>");
        let img = serve(Some(&root), "/img/frames/a%20b.png?v=2");
        assert_eq!((img.status, img.content_type), (200, "image/png"));
        assert_eq!(serve(Some(&root), "/img/frames/missing.png").status, 404);
        assert_eq!(serve(None, "/").status, 500);
    }

    #[test]
    fn refuses_paths_outside_root() {
        let root = fake_root("escape");
        fs::write(root.parent().unwrap().join("secret.txt"), "x").unwrap();
        for bad in ["/../secret.txt", "/img/../../secret.txt", "/%2e%2e/secret.txt", "/img/..%2F..%2Fsecret.txt", "/C:/Windows/win.ini", "/img\\..\\..\\secret.txt", "/%ZZ"] {
            assert!(resolve(&root, bad).is_none(), "{bad}");
        }
        assert!(resolve(&root, "/img/frames/a%20b.png").is_some());
    }

    #[test]
    fn finds_root() {
        let root = fake_root("find");
        let exe = root.join("desktop/src-tauri/target/release/app.exe");
        assert_eq!(find_root(Some(&exe), Path::new("/nowhere")), Some(root.clone()));
        assert_eq!(find_root(None, &root), Some(root.clone()));
        assert_eq!(find_root(None, Path::new("/nowhere")), None);
    }
}
