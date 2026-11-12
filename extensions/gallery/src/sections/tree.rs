//! The tree: a made-up project's files, one folder loaded only when it's
//! opened.

// gallery:begin
use fenix_ui_kit::{Canvas, Key, KeyPress, Mouse, Outcome, Rect, Role, Size, Split, Style, TreeNode, TreeState};

use crate::data::{size, FILES, FIXTURES};
use crate::draw;
use crate::sections::Reply;

pub struct State {
    pub tree: TreeState,
    /// Whether tests/fixtures has been opened, and its files read.
    pub loaded: bool,
}

/// The project's folder: every id starts with it.
const ROOT: &str = "harbour-tides/";

/// `files`' paths under the project's folder.
fn under_root(files: &[(&str, u32)]) -> Vec<(String, u32)> {
    files.iter().map(|(p, n)| (format!("{ROOT}{p}"), *n)).collect()
}

/// The folders and files under `prefix`, from paths: folders first.
fn nodes(files: &[(String, u32)], prefix: &str, loaded: bool) -> Vec<TreeNode> {
    let mut folders: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    for (path, bytes) in files {
        let Some(rest) = path.strip_prefix(prefix) else { continue };
        match rest.split_once('/') {
            Some((folder, _)) if !folders.contains(&folder) => folders.push(folder),
            Some(_) => {}
            None => out.push(TreeNode::leaf(path.as_str(), rest).detail(size(*bytes))),
        }
    }
    let mut dirs: Vec<TreeNode> = folders
        .into_iter()
        .map(|f| {
            let id = format!("{prefix}{f}/");
            let children = nodes(files, &id, loaded);
            TreeNode::branch(id, format!("{f}/"), children).style(Role::Accent)
        })
        .collect();
    if prefix == format!("{ROOT}tests/") {
        // Its fixtures come when it's opened: a lazy branch until then.
        let id = format!("{prefix}fixtures/");
        let fixtures = if loaded { TreeNode::branch(id.clone(), "fixtures/", nodes(&under_root(&FIXTURES), &id, true)) } else { TreeNode::lazy(id, "fixtures/") };
        dirs.push(fixtures.style(Role::Accent));
    }
    dirs.append(&mut out);
    dirs
}

impl State {
    pub fn new() -> State {
        let mut tree = TreeState::new();
        tree.expand(ROOT);
        tree.expand(&format!("{ROOT}src/"));
        State { tree, loaded: false }
    }

    /// The tree, from its one root.
    pub fn roots(&self) -> Vec<TreeNode> {
        vec![TreeNode::branch(ROOT, ROOT, nodes(&under_root(&FILES), ROOT, self.loaded)).style(Style::new(Role::Title))]
    }

    pub fn keys(&self) -> Vec<(&'static str, &'static str)> {
        vec![("j k", "move"), ("l", "open"), ("h", "close, or up"), ("Enter", "open, or the file"), ("+ -", "all open, closed")]
    }

    pub fn render(&self, rect: Rect, c: &mut Canvas) {
        let roots = self.roots();
        let (left, right) = Split::horizontal(Size::Percent(55)).render(rect, c);
        // A cell between it and the divider.
        let (_, left) = left.split_right(1);
        self.tree.render(&roots, left, c);
        // What's selected, beside it.
        let Some(node) = self.tree.selected_node(&roots) else { return };
        let right = right.inset_x(1);
        draw::text(c, right, 0, 0, &node.id, Role::Title);
        let what = if node.lazy { "a folder, not read yet: l reads it".to_string() } else if node.expandable() { format!("a folder of {}", node.children.len()) } else { "a file".to_string() };
        draw::text(c, right, 0, 1, &what, Role::Muted);
        if node.expandable() {
            draw::text(c, right, 0, 2, if self.tree.is_expanded(&node.id) { "open" } else { "closed" }, Role::Accent);
        } else if let Some(d) = &node.detail {
            draw::text(c, right, 0, 2, d, Role::Text);
        }
    }

    pub fn key(&mut self, press: KeyPress) -> Reply {
        let roots = self.roots();
        match press.key {
            Key::Char('+') => {
                self.loaded = true;
                fn all(s: &mut TreeState, nodes: &[TreeNode]) {
                    for n in nodes.iter().filter(|n| n.expandable()) {
                        s.expand(&n.id);
                        all(s, &n.children);
                    }
                }
                let roots = self.roots();
                all(&mut self.tree, &roots);
                Reply::Done
            }
            Key::Char('-') => {
                fn none(s: &mut TreeState, nodes: &[TreeNode]) {
                    for n in nodes {
                        s.collapse(&n.id);
                        none(s, &n.children);
                    }
                }
                none(&mut self.tree, &roots);
                self.tree.selected = 0;
                Reply::Done
            }
            key => {
                let outcome = self.tree.handle_key(key, &roots);
                self.answer(outcome, &roots)
            }
        }
    }

    /// What a key or a click did: a lazy folder opened is read; Enter on
    /// a folder opens or closes it, on a file says what it would do.
    fn answer(&mut self, outcome: Outcome, roots: &[TreeNode]) -> Reply {
        match outcome {
            Outcome::Expand => {
                if self.tree.selected_node(roots).is_some_and(|n| n.lazy) {
                    self.loaded = true;
                    return Reply::Say(format!("Read {ROOT}tests/fixtures/: {} files", FIXTURES.len()));
                }
                Reply::Done
            }
            Outcome::Submit => {
                let Some(node) = self.tree.selected_node(roots).cloned() else { return Reply::Done };
                if node.expandable() {
                    let outcome = self.tree.handle_key(if self.tree.is_expanded(&node.id) { Key::Left } else { Key::Right }, roots);
                    return self.answer(outcome, roots);
                }
                Reply::Say(format!("{}: an extension would open it here (host::open_file)", node.id))
            }
            outcome => Reply::from(outcome),
        }
    }

    pub fn mouse(&mut self, m: &Mouse) -> Reply {
        let roots = self.roots();
        let outcome = self.tree.mouse(m, &roots);
        self.answer(outcome, &roots)
    }
}
// gallery:end

impl Default for State {
    fn default() -> State {
        State::new()
    }
}
