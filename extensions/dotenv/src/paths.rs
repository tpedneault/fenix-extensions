//! Full paths, `file://` URIs, and paths below the project.

/// A `file://` URI's path: `/home/me/.env`, `C:/work/.env`.
pub fn uri_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    let path = String::from_utf8(out).ok()?;
    let b = path.as_bytes();
    Some(if b.len() > 2 && b[0] == b'/' && b[2] == b':' { path[1..].to_string() } else { path })
}

/// `path` below `root` (both full paths), with `/`: `api/.env`. A path
/// already under `/project` is below it too.
pub fn below(path: &str, root: Option<&str>) -> Option<String> {
    let path = path.replace('\\', "/");
    if let Some(rel) = path.strip_prefix("/project/") {
        return Some(rel.to_string());
    }
    let root = root?.replace('\\', "/");
    let root = root.trim_end_matches('/');
    let windows = root.as_bytes().get(1) == Some(&b':');
    let head = path.get(..root.len())?;
    let same = if windows { head.eq_ignore_ascii_case(root) } else { head == root };
    let rest = path[root.len()..].strip_prefix('/')?;
    (same && !rest.is_empty()).then(|| rest.to_string())
}

/// Where the extension reaches a file below the project.
pub fn project(rel: &str) -> String {
    format!("/project/{rel}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_and_paths_below_the_project() {
        assert_eq!(uri_path("file:///home/me/my%20app/.env").as_deref(), Some("/home/me/my app/.env"));
        assert_eq!(uri_path("file:///c%3A/work/.env").as_deref(), Some("c:/work/.env"));
        assert_eq!(below("/home/me/app/api/.env", Some("/home/me/app/")).as_deref(), Some("api/.env"));
        assert_eq!(below("/home/me/apps/.env", Some("/home/me/app")), None);
        assert_eq!(below("C:\\Work\\App\\.env", Some("c:/work/app")).as_deref(), Some(".env"));
        assert_eq!(below("/project/.env.local", None).as_deref(), Some(".env.local"));
        assert_eq!(below("/home/me/app", Some("/home/me/app")), None);
        assert_eq!(project("api/.env"), "/project/api/.env");
    }
}
