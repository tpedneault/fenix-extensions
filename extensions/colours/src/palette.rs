//! The project's palette: every colour its files use, read a few files at
//! a time (`.gitignore` kept to; binaries and big files passed over), the
//! same colours as one, grouped by hue, and the near-duplicates.

use std::collections::{HashMap, VecDeque};

use crate::colour::{delta_e_ok, Colour};
use crate::ignore::Ignore;
use crate::literal::{find, Scan};

/// The biggest file read.
pub const BIGGEST: u64 = 1 << 20;
/// The most files read, and literals kept.
pub const MOST_FILES: usize = 20_000;
pub const MOST_USES: usize = 50_000;

/// Files never read: pictures, fonts, archives, builds.
const BINARY: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "icns", "tif", "tiff", "psd", "pdf", "zip", "gz", "tgz", "xz", "bz2", "7z", "rar", "jar", "war", "woff", "woff2", "ttf", "otf", "eot", "mp3", "mp4", "mov", "avi", "webm", "wav", "ogg", "flac", "exe", "dll", "so", "dylib", "a", "o", "obj", "lib", "wasm", "class", "pyc", "bin", "dat", "db", "sqlite", "lock", "map", "fenixext",
];

/// What the walk needs of a file system: a folder's entries (name, a
/// folder or not, size), and a file's bytes. Paths are below the project.
pub trait Files {
    fn list(&self, dir: &str) -> Vec<(String, bool, u64)>;
    fn read(&self, path: &str) -> Option<Vec<u8>>;
}

/// The project at `/project`, through WASI.
pub struct Project;

impl Files for Project {
    fn list(&self, dir: &str) -> Vec<(String, bool, u64)> {
        let full = if dir.is_empty() { "/project".to_string() } else { format!("/project/{dir}") };
        let Ok(rd) = std::fs::read_dir(full) else { return Vec::new() };
        let mut out: Vec<(String, bool, u64)> = rd
            .flatten()
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                Some((e.file_name().to_string_lossy().into_owned(), meta.is_dir(), meta.len()))
            })
            .collect();
        out.sort();
        out
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(format!("/project/{path}")).ok()
    }
}

/// One place a colour is used.
#[derive(Debug, Clone, PartialEq)]
pub struct Use {
    /// Below the project.
    pub path: String,
    /// From 1.
    pub line: u32,
    /// The literal as written.
    pub text: String,
}

/// A colour and where it's used: literals that come to the same bytes
/// (`#fff`, `white`, `rgb(255 255 255)`) are one.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub colour: Colour,
    pub uses: Vec<Use>,
}

impl Entry {
    /// `#rrggbb`, or `#rrggbbaa` when it's see-through.
    pub fn hex(&self) -> String {
        let [r, g, b, a] = self.colour.bytes();
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }

    pub fn files(&self) -> usize {
        let mut paths: Vec<&str> = self.uses.iter().map(|u| u.path.as_str()).collect();
        paths.sort_unstable();
        paths.dedup();
        paths.len()
    }
}

/// A family of hues, by OKLCH's hue; greys (hardly any chroma) apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    Red,
    Orange,
    Yellow,
    Green,
    Cyan,
    Blue,
    Purple,
    Pink,
    Grey,
}

impl Family {
    pub fn of(c: &Colour) -> Family {
        let [_, chroma, h] = c.oklch();
        if chroma < 0.03 {
            return Family::Grey;
        }
        match h {
            h if (15.0..45.0).contains(&h) => Family::Red,
            h if (45.0..80.0).contains(&h) => Family::Orange,
            h if (80.0..120.0).contains(&h) => Family::Yellow,
            h if (120.0..170.0).contains(&h) => Family::Green,
            h if (170.0..225.0).contains(&h) => Family::Cyan,
            h if (225.0..280.0).contains(&h) => Family::Blue,
            h if (280.0..335.0).contains(&h) => Family::Purple,
            _ => Family::Pink,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Family::Red => "Reds",
            Family::Orange => "Oranges",
            Family::Yellow => "Yellows",
            Family::Green => "Greens",
            Family::Cyan => "Cyans",
            Family::Blue => "Blues",
            Family::Purple => "Purples",
            Family::Pink => "Pinks",
            Family::Grey => "Greys",
        }
    }
}

/// Every colour found, by its bytes.
#[derive(Debug, Clone, Default)]
pub struct Palette {
    pub entries: Vec<Entry>,
    index: HashMap<[u8; 4], usize>,
    pub uses: usize,
}

/// Near-duplicates: closer than this in OKLab (2, where 0.02 is about as
/// little as can be seen), and alphas within this.
pub const NEAR: f64 = 0.03;
const NEAR_ALPHA: f64 = 0.05;

impl Palette {
    pub fn new() -> Palette {
        Palette::default()
    }

    /// The literals of file `path`'s text.
    pub fn add_file(&mut self, path: &str, text: &str) {
        let scan = Scan::for_path(path);
        for (i, line) in text.lines().enumerate() {
            if line.len() > 400_000 {
                continue;
            }
            if !line.contains('#') && !line.contains('(') && !scan.style && !scan.theme {
                continue;
            }
            let chars: Vec<char> = line.chars().collect();
            for lit in find(line, scan) {
                if self.uses >= MOST_USES {
                    return;
                }
                let colour = lit.colour.gamut_map();
                let key = colour.bytes();
                let at = *self.index.entry(key).or_insert_with(|| {
                    self.entries.push(Entry { colour, uses: Vec::new() });
                    self.entries.len() - 1
                });
                self.entries[at].uses.push(Use { path: path.to_string(), line: i as u32 + 1, text: chars[lit.start..lit.end].iter().collect() });
                self.uses += 1;
            }
        }
    }

    /// The entries in each family, darkest first; families in hue order,
    /// greys last.
    pub fn by_hue(&self) -> Vec<(Family, Vec<usize>)> {
        let mut groups: HashMap<Family, Vec<usize>> = HashMap::new();
        for (i, e) in self.entries.iter().enumerate() {
            groups.entry(Family::of(&e.colour)).or_default().push(i);
        }
        let mut out: Vec<(Family, Vec<usize>)> = groups.into_iter().collect();
        out.sort_by_key(|(f, _)| *f);
        for (_, members) in &mut out {
            members.sort_by(|&a, &b| {
                let (x, y) = (&self.entries[a], &self.entries[b]);
                x.colour.oklch()[0].total_cmp(&y.colour.oklch()[0]).then(y.uses.len().cmp(&x.uses.len()))
            });
        }
        out
    }

    /// Colours so near each other they're likely meant to be one: groups
    /// of two or more, chained by [`NEAR`], the most used first in each,
    /// the biggest groups first.
    pub fn near_duplicates(&self) -> Vec<Vec<usize>> {
        let n = self.entries.len().min(4000);
        let labs: Vec<[f64; 3]> = self.entries[..n].iter().map(|e| e.colour.oklab()).collect();
        let mut group: Vec<usize> = (0..n).collect();
        fn root(group: &mut [usize], mut i: usize) -> usize {
            while group[i] != i {
                group[i] = group[group[i]];
                i = group[i];
            }
            i
        }
        for i in 0..n {
            for j in i + 1..n {
                let (p, q) = (labs[i], labs[j]);
                let d = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
                if d < NEAR && (self.entries[i].colour.a - self.entries[j].colour.a).abs() < NEAR_ALPHA {
                    let (a, b) = (root(&mut group, i), root(&mut group, j));
                    group[a.max(b)] = a.min(b);
                }
            }
        }
        let mut sets: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..n {
            let r = root(&mut group, i);
            sets.entry(r).or_default().push(i);
        }
        let mut out: Vec<Vec<usize>> = sets.into_values().filter(|s| s.len() > 1).collect();
        for s in &mut out {
            s.sort_by_key(|&i| std::cmp::Reverse(self.entries[i].uses.len()));
        }
        out.sort_by_key(|s| (std::cmp::Reverse(s.len()), s[0]));
        out
    }

    /// How far apart the two most different colours of a group look.
    pub fn spread(&self, group: &[usize]) -> f64 {
        let mut most: f64 = 0.0;
        for (k, &i) in group.iter().enumerate() {
            for &j in &group[k + 1..] {
                most = most.max(delta_e_ok(&self.entries[i].colour, &self.entries[j].colour));
            }
        }
        most
    }
}

/// Reading the project a few files at a time, so no call into the
/// extension runs long.
#[derive(Debug, Default)]
pub struct Walk {
    dirs: Vec<String>,
    files: VecDeque<String>,
    ignore: Ignore,
    pub read: usize,
    /// Files passed over: too big, binary, or past the most read.
    pub skipped: usize,
}

fn below(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

impl Walk {
    pub fn new() -> Walk {
        Walk { dirs: vec![String::new()], ..Walk::default() }
    }

    pub fn done(&self) -> bool {
        self.dirs.is_empty() && self.files.is_empty()
    }

    /// One folder listed, or one file read into `palette`; false once
    /// there's nothing left.
    pub fn step(&mut self, fs: &impl Files, palette: &mut Palette) -> bool {
        if let Some(path) = self.files.pop_front() {
            if self.read >= MOST_FILES || palette.uses >= MOST_USES {
                self.skipped += 1;
                return true;
            }
            let Some(bytes) = fs.read(&path) else { return true };
            let head = &bytes[..bytes.len().min(8000)];
            match (head.contains(&0), std::str::from_utf8(&bytes)) {
                (false, Ok(text)) => {
                    palette.add_file(&path, text);
                    self.read += 1;
                }
                _ => self.skipped += 1,
            }
            return true;
        }
        let Some(dir) = self.dirs.pop() else { return false };
        let entries = fs.list(&dir);
        if entries.iter().any(|(n, d, _)| n == ".gitignore" && !d) {
            if let Some(text) = fs.read(&below(&dir, ".gitignore")) {
                self.ignore.add(&dir, &String::from_utf8_lossy(&text));
            }
        }
        let mut subdirs = Vec::new();
        for (name, is_dir, size) in entries {
            let path = below(&dir, &name);
            if name == ".git" || (is_dir && name == "node_modules") || self.ignore.ignored(&path, is_dir) {
                continue;
            }
            if is_dir {
                subdirs.push(path);
                continue;
            }
            let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
            if size > BIGGEST || BINARY.contains(&ext.as_str()) || name.ends_with(".min.js") {
                self.skipped += 1;
                continue;
            }
            self.files.push_back(path);
        }
        // Depth first, in name order.
        subdirs.reverse();
        self.dirs.extend(subdirs);
        true
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A project in memory: paths and their bytes; folders are what's
    /// before a `/`.
    pub struct Memory(pub BTreeMap<String, Vec<u8>>);

    impl Files for Memory {
        fn list(&self, dir: &str) -> Vec<(String, bool, u64)> {
            let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
            let mut out: Vec<(String, bool, u64)> = Vec::new();
            for (path, bytes) in &self.0 {
                let Some(rest) = path.strip_prefix(&prefix) else { continue };
                match rest.split_once('/') {
                    Some((d, _)) => {
                        if !out.iter().any(|(n, _, _)| n == d) {
                            out.push((d.to_string(), true, 0));
                        }
                    }
                    None => out.push((rest.to_string(), false, bytes.len() as u64)),
                }
            }
            out.sort();
            out
        }

        fn read(&self, path: &str) -> Option<Vec<u8>> {
            self.0.get(path).cloned()
        }
    }

    pub fn project() -> Memory {
        let mut m = BTreeMap::new();
        let mut put = |p: &str, s: &[u8]| m.insert(p.to_string(), s.to_vec());
        put(".gitignore", b"dist/\n*.log\n");
        put("src/app.css", b"a { color: #3b82f6; }\nb { color: rgb(59 130 246); background: white }\nc { border-color: #3b83f6 }\n");
        put("src/main.rs", b"// Part of #27\nconst ACCENT: &str = \"#ff0000\";\nconst MUTED: &str = \"#FFF\";\n");
        put("dist/app.css", b"a { color: #00ff00 }\n");
        put("debug.log", b"#00ff00\n");
        put("logo.png", b"\x89PNG #00ff00");
        put("data.txt", b"#00ff00\0\0");
        put("themes/dusk.toml", b"[surfaces]\nselection = { color = \"#7c7f93\", alpha = 0.25 }\n");
        put("node_modules/x.css", b"a { color: #00ff00 }\n");
        Memory(m)
    }

    #[test]
    fn the_walk_reads_what_git_keeps() {
        let fs = project();
        let mut walk = Walk::new();
        let mut p = Palette::new();
        let mut steps = 0;
        while walk.step(&fs, &mut p) {
            steps += 1;
        }
        assert!(walk.done() && steps > 4);
        assert!(p.entries.iter().all(|e| e.colour.rgb() != (0, 255, 0)), "dist, logs, pictures, binaries and node_modules left out");
        assert_eq!(walk.read, 4, ".gitignore, app.css, main.rs, dusk.toml");
        assert_eq!(walk.skipped, 2, "the picture and the binary");
        let blue = p.entries.iter().find(|e| e.hex() == "#3b82f6").unwrap();
        assert_eq!(blue.uses.iter().map(|u| (u.path.as_str(), u.line, u.text.as_str())).collect::<Vec<_>>(), [("src/app.css", 1, "#3b82f6"), ("src/app.css", 2, "rgb(59 130 246)")]);
        assert_eq!(blue.files(), 1);
        let white = p.entries.iter().find(|e| e.hex() == "#ffffff").unwrap();
        assert_eq!(white.uses.len(), 2, "white and #FFF, as one");
        assert!(p.entries.iter().any(|e| e.hex() == "#7c7f9340"), "a theme's colour with its alpha");
        assert_eq!(p.uses, 7);
    }

    #[test]
    fn grouped_by_hue_and_near_duplicates() {
        let fs = project();
        let mut walk = Walk::new();
        let mut p = Palette::new();
        while walk.step(&fs, &mut p) {}
        let groups = p.by_hue();
        let names: Vec<&str> = groups.iter().map(|(f, _)| f.name()).collect();
        assert_eq!(names, ["Reds", "Blues", "Greys"]);
        let hexes = |g: &[usize]| g.iter().map(|&i| p.entries[i].hex()).collect::<Vec<_>>();
        assert_eq!(hexes(&groups[2].1), ["#ffffff"]);
        assert_eq!(hexes(&groups[1].1), ["#7c7f9340", "#3b82f6", "#3b83f6"], "a greyish blue among the blues; darkest first");
        let near = p.near_duplicates();
        assert_eq!(near.len(), 1);
        let hexes: Vec<String> = near[0].iter().map(|&i| p.entries[i].hex()).collect();
        assert_eq!(hexes, ["#3b82f6", "#3b83f6"], "the most used first");
        assert!(p.spread(&near[0]) > 0.0 && p.spread(&near[0]) < NEAR);
    }

    #[test]
    fn families() {
        let f = |r, g, b| Family::of(&Colour::from_u8(r, g, b));
        assert_eq!(f(255, 0, 0), Family::Red);
        assert_eq!(f(255, 165, 0), Family::Orange);
        assert_eq!(f(255, 255, 0), Family::Yellow);
        assert_eq!(f(0, 128, 0), Family::Green);
        assert_eq!(f(0, 255, 255), Family::Cyan);
        assert_eq!(f(0, 0, 255), Family::Blue);
        assert_eq!(f(128, 0, 128), Family::Purple);
        assert_eq!(f(255, 105, 180), Family::Pink);
        assert_eq!(f(120, 120, 125), Family::Grey);
    }
}
