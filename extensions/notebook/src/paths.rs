//! Where things are, two ways: as the extension reaches them inside its
//! sandbox (`/folders/notebook/notes/a.md`, `/project/src/x.c`) and as
//! Fenix and the other extensions name them on this machine
//! (`C:\Users\me\notes\notes\a.md`). A file's `file:` URI is the second.

use std::path::{Path, PathBuf};

/// Where the notebook is mounted (`permissions.folders`).
pub const ROOT: &str = "/folders/notebook";

/// Where the project you're in is mounted.
pub const PROJECT: &str = "/project";

/// A path as one spelling: `/` between parts, no `/` at the end.
pub fn slashed(p: &str) -> String {
    p.replace('\\', "/").trim_end_matches('/').to_string()
}

/// Whether two spellings are the same file (Windows paths ignoring case).
pub fn same(a: &str, b: &str) -> bool {
    let (a, b) = (slashed(a), slashed(b));
    if is_windows(&a) || is_windows(&b) {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

/// `C:/...` or `\\server\...`.
fn is_windows(p: &str) -> bool {
    let b = p.as_bytes();
    (b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic()) || p.starts_with("//")
}

/// A full path on this machine rather than one inside the sandbox:
/// `C:\...`, or `/...` that isn't one of the sandbox's own folders.
pub fn is_host_absolute(p: &str) -> bool {
    let s = slashed(p);
    is_windows(&s) || (s.starts_with('/') && !inside(&s, "/folders") && !inside(&s, PROJECT) && !inside(&s, "/data") && !inside(&s, "/fonts"))
}

/// Whether `p` is `dir` or under it.
fn inside(p: &str, dir: &str) -> bool {
    p == dir || p.starts_with(&format!("{dir}/"))
}

/// What's after `dir` in `p`, if `p` is under it (`""` for `dir` itself).
fn under(p: &str, dir: &str) -> Option<String> {
    let (p, dir) = (slashed(p), slashed(dir));
    if dir.is_empty() {
        return None;
    }
    let windows = is_windows(&dir);
    let (pl, dl) = if windows { (p.to_lowercase(), dir.to_lowercase()) } else { (p.clone(), dir.clone()) };
    if pl == dl {
        return Some(String::new());
    }
    pl.starts_with(&format!("{dl}/")).then(|| p[dir.len() + 1..].to_string())
}

/// The two roots it maps between: the notebook's folder and the
/// project's, as this machine names them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Places {
    /// The notebook's folder: your `notebook.folder`, or the one in
    /// Fenix's data folder (Fenix gives a folder's setting as where it is).
    pub notebook: Option<String>,
    /// The project you're in (`here`, a language server's root).
    pub project: Option<String>,
}

impl Places {
    /// A full path on this machine as the sandbox reaches it.
    pub fn guest(&self, host: &str) -> Option<PathBuf> {
        if !is_host_absolute(host) {
            return Some(PathBuf::from(slashed(host)));
        }
        if let Some(rest) = self.notebook.as_deref().and_then(|root| under(host, root)) {
            return Some(join(ROOT, &rest));
        }
        if let Some(rest) = self.project.as_deref().and_then(|root| under(host, root)) {
            return Some(join(PROJECT, &rest));
        }
        None
    }

    /// A sandbox path as this machine names it: what Fenix and the other
    /// extensions are given.
    pub fn host(&self, guest: &Path) -> Option<String> {
        let g = slashed(&guest.to_string_lossy());
        if is_host_absolute(&g) {
            return Some(g);
        }
        let (root, rest) = if let Some(rest) = under(&g, ROOT) {
            (self.notebook.as_deref()?, rest)
        } else {
            (self.project.as_deref()?, under(&g, PROJECT)?)
        };
        let sep = if root.contains('\\') { "\\" } else { "/" };
        let root = root.trim_end_matches(['/', '\\']);
        Some(if rest.is_empty() { root.to_string() } else { format!("{root}{sep}{}", rest.replace('/', sep)) })
    }

    /// The path for Fenix, else the sandbox's as it is.
    pub fn shown(&self, guest: &Path) -> String {
        self.host(guest).unwrap_or_else(|| guest.to_string_lossy().into_owned())
    }
}

fn join(root: &str, rest: &str) -> PathBuf {
    if rest.is_empty() {
        PathBuf::from(root)
    } else {
        PathBuf::from(format!("{root}/{rest}"))
    }
}

/// A `file:` URI as a path on this machine.
pub fn uri_path(uri: &str) -> String {
    let rest = uri.strip_prefix("file:///").map(|r| if r.as_bytes().get(1) == Some(&b':') { r.to_string() } else { format!("/{r}") }).unwrap_or_else(|| uri.trim_start_matches("file://").to_string());
    let bytes = rest.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A path on this machine as a `file:` URI.
pub fn path_uri(path: &str) -> String {
    let p = slashed(path);
    let mut out = String::from(if p.starts_with('/') { "file://" } else { "file:///" });
    for b in p.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `bytes` as base64: a picture inside an HTML page.
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Base64 back to bytes: what the diagrams extension sends.
pub fn unbase64(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = B64.iter().position(|&b| b == c).ok_or("not base64")? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// `to`, relative to folder `from`, with `/`.
pub fn relative_path(from: &Path, to: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to_c: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to_c).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return to.display().to_string().replace('\\', "/");
    }
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_string(), from.len() - common).collect();
    parts.extend(to_c[common..].iter().map(|c| c.as_os_str().to_string_lossy().to_string()));
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_map_between_the_sandbox_and_the_machine() {
        let places = Places { notebook: Some("C:\\Users\\me\\Notes".into()), project: Some("C:/src/fenix-sat".into()) };
        assert_eq!(places.guest("c:/users/me/notes/notes/a.md"), Some(PathBuf::from("/folders/notebook/notes/a.md")));
        assert_eq!(places.guest("C:\\src\\fenix-sat\\docs\\x.md"), Some(PathBuf::from("/project/docs/x.md")));
        assert_eq!(places.guest("D:/other.md"), None);
        assert_eq!(places.host(Path::new("/folders/notebook/notes/a.md")).as_deref(), Some("C:\\Users\\me\\Notes\\notes\\a.md"));
        assert_eq!(places.host(Path::new("/project/docs/x.md")).as_deref(), Some("C:/src/fenix-sat/docs/x.md"));
        assert_eq!(places.host(Path::new("/folders/notebook")).as_deref(), Some("C:\\Users\\me\\Notes"));
        let linux = Places { notebook: Some("/home/me/notes".into()), project: None };
        assert_eq!(linux.guest("/home/me/notes/journal/2026/2026-10-28.md"), Some(PathBuf::from("/folders/notebook/journal/2026/2026-10-28.md")));
        assert_eq!(linux.host(Path::new("/folders/notebook/a.md")).as_deref(), Some("/home/me/notes/a.md"));
        assert!(!is_host_absolute("/folders/notebook/a.md") && is_host_absolute("/home/me/a.md") && is_host_absolute("C:\\a.md"));
    }

    #[test]
    fn uris_are_paths() {
        assert_eq!(uri_path("file:///C:/Users/me/My%20Notes/a.md"), "C:/Users/me/My Notes/a.md");
        assert_eq!(uri_path("file:///home/me/a.md"), "/home/me/a.md");
        assert_eq!(path_uri("C:\\Users\\me\\My Notes\\a.md"), "file:///C:/Users/me/My%20Notes/a.md");
        assert_eq!(path_uri("/home/me/a.md"), "file:///home/me/a.md");
        assert!(same("C:\\Users\\Me\\a.md", "c:/users/me/a.md"));
    }

    #[test]
    fn base64_both_ways() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(unbase64("TWFu").unwrap(), b"Man");
        assert_eq!(unbase64("TWE=").unwrap(), b"Ma");
        assert_eq!(unbase64(&base64(&[0, 255, 7, 9])).unwrap(), [0, 255, 7, 9]);
    }

    #[test]
    fn relative_paths() {
        assert_eq!(relative_path(Path::new("/nb/notes"), Path::new("/nb/attachments/x/a.png")), "../attachments/x/a.png");
        assert_eq!(relative_path(Path::new("/nb/notes"), Path::new("/nb/notes/a.png")), "a.png");
    }
}
