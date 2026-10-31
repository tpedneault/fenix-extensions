//! The project's own definitions, from Universal Ctags: what `ctags` is
//! asked, how its output reads, and the paths between Fenix's URIs and
//! the files ctags names.

/// One `proc`/`namespace` definition ctags found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// The fully-qualified name, `myns::subns::greet` for a `proc greet`
    /// nested inside `namespace eval myns { namespace eval subns {...} }`,
    /// or plain `global_proc` at the top level -- never a leading `::`.
    pub name: String,
    /// The file, as ctags wrote it: a full path, `/` between folders.
    pub file: String,
    /// From 1; 0 when ctags didn't say.
    pub line: u32,
    /// A `namespace eval`, not a `proc`.
    pub namespace: bool,
}

/// What ctags is run with to scan `root` for Tcl definitions. Your own
/// `~/.ctags.d` and the project's `.ctags.d` / `.ctags` are left out
/// (`--options=NONE`, which has to come first): one that asks for long
/// kind names, excludes folders or another output format leaves nothing
/// here to read. `.tm` (Tcl Modules) isn't in ctags' own Tcl mapping, and
/// an option after the folder is silently ignored, so the folder comes
/// last.
pub fn args(root: &str) -> Vec<String> {
    ["--options=NONE", "--fields=+n", "--languages=Tcl", "--langmap=Tcl:+.tm", "-R", "-f", "-"].iter().map(|s| s.to_string()).chain([windows_friendly_path(root)]).collect()
}

/// `root` without Windows' verbatim prefix (`\\?\`, `\\?\UNC\`): ctags
/// handed one scans nothing, silently.
pub fn windows_friendly_path(root: &str) -> String {
    if let Some(rest) = root.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = root.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        root.to_string()
    }
}

/// Plain vi-compatible ctags output: `{name}\t{file}\t{ex-command};"\t{kind}\t{extra-fields...}`.
/// Only procedures (`p`) and namespaces (`n`); pseudo-tag lines (`!_`)
/// and lines too short to be a tag are skipped.
pub fn parse(text: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    for raw in text.lines() {
        if raw.starts_with("!_") {
            continue;
        }
        let fields: Vec<&str> = raw.split('\t').collect();
        if fields.len() < 4 {
            continue;
        }
        let (name, file) = (fields[0], fields[1]);
        // The kind's letter, or its name (`--fields=+K`), either bare or as
        // `kind:` (`--fields=+k`).
        let kind = fields[3].strip_prefix("kind:").unwrap_or(fields[3]);
        if !matches!(kind, "p" | "n" | "procedure" | "namespace") {
            continue;
        }
        let line = fields[4..].iter().find_map(|f| f.strip_prefix("line:")).and_then(|n| n.parse().ok()).unwrap_or(0);
        let namespace = fields[4..].iter().find_map(|f| f.strip_prefix("namespace:"));
        let name = match namespace {
            Some(ns) if !ns.is_empty() => format!("{}::{name}", ns.trim_start_matches("::")),
            _ => name.to_string(),
        };
        tags.push(Tag { name, file: file.replace('\\', "/"), line, namespace: matches!(kind, "n" | "namespace") });
    }
    tags
}

/// A folder or file as one spelling: `/` between folders, no trailing
/// `/`, a Windows drive letter in capitals -- so a root from a URI and one
/// from `here` compare equal.
pub fn normalize(path: &str) -> String {
    let path = windows_friendly_path(path).replace('\\', "/");
    let path = if path.len() > 1 { path.trim_end_matches('/').to_string() } else { path };
    let mut chars = path.chars();
    match (chars.next(), chars.next()) {
        (Some(drive), Some(':')) if drive.is_ascii_alphabetic() => format!("{}{}", drive.to_ascii_uppercase(), &path[1..]),
        _ => path,
    }
}

/// A `file://` URI as a path, the way Fenix writes them: `file:///C:/x`
/// is `C:/x`, `file:///home/x` is `/home/x`.
pub fn uri_to_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let decoded = percent_decode(rest);
    let bytes = decoded.as_bytes();
    let path = if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' { decoded[1..].to_string() } else { decoded };
    Some(normalize(&path))
}

/// A full path as a `file://` URI, as Fenix makes them.
pub fn path_to_uri(path: &str) -> String {
    let path = normalize(path);
    let path = if path.starts_with('/') { path } else { format!("/{path}") };
    let mut uri = String::from("file://");
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        uri.push('/');
        for byte in segment.bytes() {
            let keep = byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@".contains(&byte);
            if keep {
                uri.push(byte as char);
            } else {
                uri.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    if uri == "file://" {
        uri.push('/');
    }
    uri
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(byte) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `file` (a full path) as a path under `root`, `/` between folders: what
/// `/project/...` is for it. `None` when it's elsewhere.
pub fn relative_to<'a>(file: &'a str, root: &str) -> Option<&'a str> {
    // A Windows path: its drive letter in either case.
    let rest = if root.as_bytes().get(1) == Some(&b':') {
        let (head, rest) = (file.get(..root.len())?, &file[root.len()..]);
        head.eq_ignore_ascii_case(root).then_some(rest)?
    } else {
        file.strip_prefix(root)?
    };
    rest.strip_prefix('/').filter(|r| !r.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctags_is_asked_for_tcl_and_tm_files_with_the_folder_last() {
        let args = args(r"\\?\C:\work\proj");
        assert_eq!(args.last().map(String::as_str), Some(r"C:\work\proj"), "no verbatim prefix");
        assert!(args.contains(&"--langmap=Tcl:+.tm".to_string()) && args.contains(&"--languages=Tcl".to_string()));
        assert_eq!(&args[args.len() - 3..args.len() - 1], ["-f", "-"], "to stdout");
        assert_eq!(args[0], "--options=NONE", "no ~/.ctags.d or project .ctags.d, and first, as ctags needs");
    }

    #[test]
    fn long_kind_names_are_read_too() {
        // A ctags configuration with --fields=+K, or +k.
        let text = "greet\t/p/a.tcl\t/^proc greet {} {$/;\"\tprocedure\tline:2\tnamespace:myns\n\
                    myns\t/p/a.tcl\t/^namespace eval myns {$/;\"\tkind:namespace\tline:1\n\
                    x\t/p/a.tcl\t/^set x 1$/;\"\tvariable\tline:9\n";
        let names: Vec<String> = parse(text).into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["myns::greet", "myns"]);
    }

    #[test]
    fn windows_friendly_path_strips_the_verbatim_prefixes() {
        assert_eq!(windows_friendly_path(r"\\?\C:\Users\thoma\project"), r"C:\Users\thoma\project");
        assert_eq!(windows_friendly_path(r"\\?\UNC\server\share\project"), r"\\server\share\project");
        assert_eq!(windows_friendly_path(r"C:\Users\thoma\project"), r"C:\Users\thoma\project");
    }

    #[test]
    fn procs_and_namespaces_are_read_with_their_qualified_names() {
        let text = "!_TAG_FILE_FORMAT\t2\t/extended format/\textra\n\
                    myns\tC:\\p\\foo.tcl\t/^namespace eval myns {$/;\"\tn\tline:1\n\
                    greet\tC:\\p\\foo.tcl\t/^    proc greet {name} {$/;\"\tp\tline:2\tnamespace:myns\n\
                    deep\t/p/foo.tcl\t/^proc deep {} {$/;\"\tp\tline:3\tnamespace:::outer::inner\n\
                    top_level_proc\t/p/foo.tcl\t/^proc top_level_proc {} {$/;\"\tp\tline:7\n\
                    x\t/p/foo.tcl\t/^set x 1$/;\"\tv\tline:9\n\
                    short\tline\n";
        let tags = parse(text);
        let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["myns", "myns::greet", "outer::inner::deep", "top_level_proc"], "no pseudo-tags, variables or short lines; no leading ::");
        assert_eq!(tags[1].file, "C:/p/foo.tcl");
        assert_eq!(tags[1].line, 2);
        assert_eq!(tags[3].line, 7);
    }

    #[test]
    fn a_tag_with_no_line_field_is_at_line_zero() {
        assert_eq!(parse("foo\tbar.tcl\t/^proc foo {} {$/;\"\tp\n"), [Tag { name: "foo".into(), file: "bar.tcl".into(), line: 0, namespace: false }]);
    }

    #[test]
    fn uris_and_paths_go_both_ways_as_fenix_writes_them() {
        assert_eq!(uri_to_path("file:///C:/Users/me/My%20Project").as_deref(), Some("C:/Users/me/My Project"));
        assert_eq!(uri_to_path("file:///c%3A/Users/me").as_deref(), Some("C:/Users/me"));
        assert_eq!(uri_to_path("file:///home/me/proj/").as_deref(), Some("/home/me/proj"));
        assert_eq!(uri_to_path("untitled:1"), None);
        assert_eq!(path_to_uri("C:/Users/me/My Project/a.tcl"), "file:///C:/Users/me/My%20Project/a.tcl");
        assert_eq!(path_to_uri(r"C:\Users\me\a.tcl"), "file:///C:/Users/me/a.tcl");
        assert_eq!(path_to_uri("/home/me/a.tcl"), "file:///home/me/a.tcl");
    }

    #[test]
    fn a_root_reads_the_same_however_it_was_spelt() {
        assert_eq!(normalize(r"c:\Users\me\proj\"), "C:/Users/me/proj");
        assert_eq!(normalize(r"\\?\C:\Users\me\proj"), "C:/Users/me/proj");
        assert_eq!(normalize("/home/me/proj/"), "/home/me/proj");
        assert_eq!(normalize("/"), "/");
    }

    #[test]
    fn a_file_under_the_root_is_found_relative_to_it() {
        assert_eq!(relative_to("C:/p/lib/a.tcl", "C:/p"), Some("lib/a.tcl"));
        assert_eq!(relative_to("c:/p/a.tcl", "C:/p"), Some("a.tcl"), "a drive letter in either case");
        assert_eq!(relative_to("/home/p/a.tcl", "/home/p"), Some("a.tcl"));
        assert_eq!(relative_to("/home/px/a.tcl", "/home/p"), None);
        assert_eq!(relative_to("/elsewhere/a.tcl", "/home/p"), None);
    }
}
