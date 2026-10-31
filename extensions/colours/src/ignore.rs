//! What a project's `.gitignore` files leave out, as Git reads them:
//! patterns by folder, `!` taking one back, `/` anchoring one, a
//! trailing `/` for folders only, `*`, `?`, `[a-z]` and `**`.

/// One line of a `.gitignore`.
#[derive(Debug, Clone, PartialEq)]
struct Rule {
    /// The folder its file is in, below the project ("" at its root),
    /// with a `/` after when it isn't the root.
    base: String,
    pattern: Vec<char>,
    negate: bool,
    dir_only: bool,
    /// Matched against the whole path below `base`, not just a name.
    anchored: bool,
}

/// The rules read so far, in the order Git weighs them: later ones win.
#[derive(Debug, Clone, Default)]
pub struct Ignore {
    rules: Vec<Rule>,
}

impl Ignore {
    /// The rules of the `.gitignore` in folder `base` (below the project,
    /// "" its root).
    pub fn add(&mut self, base: &str, text: &str) {
        let base = if base.is_empty() { String::new() } else { format!("{}/", base.trim_end_matches('/')) };
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            // Trailing spaces go, unless escaped.
            let line = if line.ends_with("\\ ") { line } else { line.trim_end() };
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (negate, rest) = match line.strip_prefix('!') {
                Some(r) => (true, r),
                None => (false, line.strip_prefix('\\').filter(|r| r.starts_with('!') || r.starts_with('#')).unwrap_or(line)),
            };
            let (dir_only, rest) = match rest.strip_suffix('/') {
                Some(r) => (true, r),
                None => (false, rest),
            };
            let anchored = rest.contains('/');
            let rest = rest.strip_prefix('/').unwrap_or(rest);
            if rest.is_empty() {
                continue;
            }
            self.rules.push(Rule { base: base.clone(), pattern: rest.chars().collect(), negate, dir_only, anchored });
        }
    }

    /// Whether `path` (below the project, `/` between its parts) is
    /// ignored. A file in an ignored folder is: the walk doesn't go into
    /// one, so it's asked of the folder.
    pub fn ignored(&self, path: &str, is_dir: bool) -> bool {
        let mut ignored = false;
        for rule in &self.rules {
            if rule.dir_only && !is_dir {
                continue;
            }
            let Some(below) = path.strip_prefix(rule.base.as_str()) else { continue };
            let hit = if rule.anchored {
                glob(&rule.pattern, &below.chars().collect::<Vec<_>>())
            } else {
                let name = below.rsplit('/').next().unwrap_or(below);
                glob(&rule.pattern, &name.chars().collect::<Vec<_>>())
            };
            if hit {
                ignored = !rule.negate;
            }
        }
        ignored
    }
}

/// Whether `text` matches glob `pat`: `*` and `?` within a part of the
/// path, `**` across them, `[...]` a class.
fn glob(pat: &[char], text: &[char]) -> bool {
    match pat.first() {
        None => text.is_empty(),
        Some('*') if pat.get(1) == Some(&'*') => {
            // `**/` any folders, `/**` everything in it, `**` anything.
            let rest = &pat[2..];
            let rest = rest.strip_prefix(&['/']).unwrap_or(rest);
            if rest.is_empty() {
                return true;
            }
            (0..=text.len()).any(|i| (i == 0 || text[i - 1] == '/') && glob(rest, &text[i..]))
        }
        Some('*') => (0..=text.len()).take_while(|&i| i == 0 || text[i - 1] != '/').any(|i| glob(&pat[1..], &text[i..])),
        Some('?') => text.first().is_some_and(|&c| c != '/') && glob(&pat[1..], &text[1..]),
        Some('[') => {
            let Some(close) = pat.iter().skip(2).position(|&c| c == ']').map(|p| p + 2) else {
                return text.first() == Some(&'[') && glob(&pat[1..], &text[1..]);
            };
            let Some(&c) = text.first() else { return false };
            let class = &pat[1..close];
            let (neg, class) = match class.first() {
                Some('!' | '^') => (true, &class[1..]),
                _ => (false, class),
            };
            let mut hit = false;
            let mut i = 0;
            while i < class.len() {
                if i + 2 < class.len() && class[i + 1] == '-' {
                    hit |= (class[i]..=class[i + 2]).contains(&c);
                    i += 3;
                } else {
                    hit |= class[i] == c;
                    i += 1;
                }
            }
            hit != neg && c != '/' && glob(&pat[close + 1..], &text[1..])
        }
        Some('\\') if pat.len() > 1 => text.first() == Some(&pat[1]) && glob(&pat[2..], &text[1..]),
        Some(&p) => text.first() == Some(&p) && glob(&pat[1..], &text[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(p: &str, t: &str) -> bool {
        glob(&p.chars().collect::<Vec<_>>(), &t.chars().collect::<Vec<_>>())
    }

    #[test]
    fn globs() {
        assert!(g("*.css", "a.css") && !g("*.css", "a.scss") && !g("*.css", "d/a.css"));
        assert!(g("a?c", "abc") && !g("a?c", "a/c"));
        assert!(g("[a-c]x", "bx") && !g("[!a-c]x", "bx") && g("[!a-c]x", "dx"));
        assert!(g("**/build", "build") && g("**/build", "a/b/build"));
        assert!(g("doc/**", "doc/a/b.md") && g("a/**/b", "a/b") && g("a/**/b", "a/x/y/b") && !g("a/**/b", "ab"));
        assert!(g("\\#x", "#x"));
    }

    #[test]
    fn a_projects_gitignore() {
        let mut ig = Ignore::default();
        ig.add("", "# built\ntarget/\n*.min.css\n!keep.min.css\n/dist\nnode_modules\ndocs/*.html\n");
        ig.add("web", "cache\n/local.css\n");
        assert!(ig.ignored("target", true));
        assert!(!ig.ignored("target", false), "a file called target isn't a folder");
        assert!(ig.ignored("a/b/target", true), "unanchored: at any depth");
        assert!(ig.ignored("style.min.css", false) && ig.ignored("x/app.min.css", false));
        assert!(!ig.ignored("keep.min.css", false), "taken back");
        assert!(ig.ignored("dist", true) && !ig.ignored("src/dist", true), "anchored at the root");
        assert!(ig.ignored("web/node_modules", true));
        assert!(ig.ignored("docs/a.html", false) && !ig.ignored("docs/x/a.html", false));
        assert!(ig.ignored("web/cache", true) && ig.ignored("web/a/cache", false) && !ig.ignored("cache", false), "a folder's rules for what's under it");
        assert!(ig.ignored("web/local.css", false) && !ig.ignored("web/a/local.css", false));
        assert!(!ig.ignored("src/app.css", false));
    }
}
