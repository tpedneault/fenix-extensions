//! What Discord is shown: the activity built from where you are in the
//! editor and the privacy settings, in plain Rust.

use serde_json::{json, Map, Value};

/// The asset Fenix's own picture is uploaded as.
pub const FENIX: &str = "fenix";
/// The small picture while you're idle.
pub const IDLE: &str = "idle";
/// A file of no language below.
pub const TEXT: &str = "text";
/// The small picture while you're debugging.
pub const DEBUG: &str = "debug";
/// The small picture while a task runs.
pub const TASK: &str = "task";

/// By file extension (or name, for those that have none): the asset key
/// of the language's picture, and its name.
const LANGUAGES: &[(&[&str], &str, &str)] = &[
    (&["rs"], "rust", "Rust"),
    (&["py", "pyi", "pyw"], "python", "Python"),
    (&["js", "mjs", "cjs", "jsx"], "javascript", "JavaScript"),
    (&["ts", "mts", "cts", "tsx"], "typescript", "TypeScript"),
    (&["c", "h"], "c", "C"),
    (&["cpp", "cc", "cxx", "c++", "hpp", "hh", "hxx", "h++", "ipp"], "cpp", "C++"),
    (&["cs", "csx"], "csharp", "C#"),
    (&["go"], "go", "Go"),
    (&["java"], "java", "Java"),
    (&["kt", "kts"], "kotlin", "Kotlin"),
    (&["swift"], "swift", "Swift"),
    (&["rb", "rake", "gemspec"], "ruby", "Ruby"),
    (&["php"], "php", "PHP"),
    (&["lua"], "lua", "Lua"),
    (&["zig"], "zig", "Zig"),
    (&["hs", "lhs"], "haskell", "Haskell"),
    (&["ml", "mli"], "ocaml", "OCaml"),
    (&["ex", "exs"], "elixir", "Elixir"),
    (&["erl", "hrl"], "erlang", "Erlang"),
    (&["clj", "cljs", "cljc", "edn"], "clojure", "Clojure"),
    (&["scala", "sc"], "scala", "Scala"),
    (&["dart"], "dart", "Dart"),
    (&["r"], "r", "R"),
    (&["jl"], "julia", "Julia"),
    (&["sh", "bash", "zsh", "fish"], "shell", "Shell"),
    (&["ps1", "psm1", "psd1"], "powershell", "PowerShell"),
    (&["tcl", "tm"], "tcl", "Tcl"),
    (&["html", "htm"], "html", "HTML"),
    (&["css"], "css", "CSS"),
    (&["scss", "sass"], "sass", "Sass"),
    (&["vue"], "vue", "Vue"),
    (&["svelte"], "svelte", "Svelte"),
    (&["json", "jsonc", "json5"], "json", "JSON"),
    (&["yaml", "yml"], "yaml", "YAML"),
    (&["toml"], "toml", "TOML"),
    (&["xml", "xsd", "xsl", "svg"], "xml", "XML"),
    (&["md", "markdown"], "markdown", "Markdown"),
    (&["tex", "sty", "cls"], "latex", "LaTeX"),
    (&["sql"], "sql", "SQL"),
    (&["nix"], "nix", "Nix"),
    (&["ino"], "arduino", "Arduino"),
    (&["mmd", "mermaid"], "mermaid", "Mermaid"),
    (&["cmake", "CMakeLists.txt"], "cmake", "CMake"),
    (&["Dockerfile", "Containerfile", "dockerfile"], "docker", "Dockerfile"),
    (&["Makefile", "GNUmakefile", "makefile", "mk"], "make", "Makefile"),
    (&["csv", "tsv"], "csv", "CSV"),
];

/// A file's language: its picture's asset key and its name, or `text`
/// and none for one of no language here.
pub fn language(path: &str) -> (&'static str, Option<&'static str>) {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let ext = name.rsplit_once('.').filter(|(stem, _)| !stem.is_empty()).map(|(_, ext)| ext.to_ascii_lowercase());
    LANGUAGES
        .iter()
        .find(|(names, ..)| names.contains(&name) || ext.as_deref().is_some_and(|e| names.iter().any(|n| !n.contains('.') && n.eq_ignore_ascii_case(e))))
        .map(|(_, key, lang)| (*key, Some(*lang)))
        .unwrap_or((TEXT, None))
}

/// A language's name by Fenix's name for it (`cpp`, as a debug session
/// says it): none for one not here.
pub fn language_named(fenix: &str) -> Option<&'static str> {
    let fenix = fenix.trim().to_ascii_lowercase();
    LANGUAGES.iter().find(|(names, key, _)| *key == fenix || names.contains(&fenix.as_str())).map(|(_, _, lang)| *lang)
}

/// A debug session, as Fenix tells of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: u64,
    /// What it debugs: a program, or its launch's name.
    pub name: String,
    /// Fenix's name for its language.
    pub language: String,
    /// The session it was started from, for one an adapter asked for.
    pub parent: Option<u64>,
    /// Stopped where it was, and why (`breakpoint`).
    pub paused: Option<String>,
    pub since: u64,
}

/// What became of a debug session: Fenix's `debug-state`, in plain Rust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Debug {
    Started { id: u64, name: String, language: String, parent: Option<u64> },
    Paused(u64, String),
    Resumed(u64),
    Ended(u64),
}

/// The project you're in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    /// Its folder, a full path.
    pub root: String,
}

/// From when the elapsed time counts (`discord.elapsed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Elapsed {
    /// Since you came to this project.
    #[default]
    Project,
    /// Since you came to this file.
    File,
    None,
}

impl Elapsed {
    pub fn parse(text: &str) -> Elapsed {
        match text.trim() {
            "file" => Elapsed::File,
            "none" => Elapsed::None,
            _ => Elapsed::Project,
        }
    }
}

/// What may be shown.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Privacy {
    /// "Editing a Rust file", not its name.
    pub hide_files: bool,
    /// No project named.
    pub hide_projects: bool,
    /// Projects nothing is shown for: their names or folders, `*` for any
    /// run of characters.
    pub private: Vec<String>,
    pub elapsed: Elapsed,
}

impl Privacy {
    /// Whether nothing's shown for `project`.
    pub fn is_private(&self, project: &Project) -> bool {
        let root = project.root.replace('\\', "/");
        let root = root.trim_end_matches('/');
        self.private.iter().map(|p| p.trim().replace('\\', "/")).filter(|p| !p.is_empty()).any(|p| {
            let p = p.trim_end_matches('/');
            glob(p, &project.name) || glob(p, root)
        })
    }
}

/// Whether `text` is `pattern`, ignoring case, `*` any run of characters.
fn glob(pattern: &str, text: &str) -> bool {
    fn go(p: &[char], t: &[char]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some(('*', rest)) => (0..=t.len()).any(|i| go(rest, &t[i..])),
            Some((c, rest)) => t.first().is_some_and(|d| d.to_lowercase().eq(c.to_lowercase())) && go(rest, &t[1..]),
        }
    }
    go(&pattern.chars().collect::<Vec<_>>(), &text.chars().collect::<Vec<_>>())
}

/// Where you are, and since when: times in ms since 1970.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Editor {
    /// The file you're in, a full path: the last one, while the focus is
    /// on a page or a terminal.
    pub file: Option<String>,
    pub project: Option<Project>,
    /// When you came to the project, and to the file.
    pub project_since: u64,
    pub file_since: u64,
    /// When you last typed, or pressed a key.
    pub input: u64,
    pub idle: bool,
    /// The task running in the task pane, and since when.
    pub task: Option<(String, u64)>,
    /// The debug sessions there are, in the order they started.
    pub sessions: Vec<Session>,
}

impl Editor {
    pub fn new(now: u64) -> Editor {
        Editor { project_since: now, file_since: now, input: now, ..Editor::default() }
    }

    /// You're in `file`, of `project`; whether that's a change.
    pub fn moved(&mut self, file: &str, project: Option<Project>, now: u64) -> bool {
        if self.file.as_deref() == Some(file) && self.project == project {
            return false;
        }
        if self.project.as_ref().map(|p| &p.root) != project.as_ref().map(|p| &p.root) {
            self.project_since = now;
        }
        if self.file.as_deref() != Some(file) {
            self.file_since = now;
        }
        self.file = Some(file.to_string());
        self.project = project;
        true
    }

    /// A key: whether you were idle till now.
    pub fn typed(&mut self, now: u64) -> bool {
        self.input = now;
        std::mem::take(&mut self.idle)
    }

    /// A task started in the task pane (one runs at a time).
    pub fn task_started(&mut self, name: &str, now: u64) -> bool {
        self.task = Some((name.to_string(), now));
        true
    }

    /// The task ended: whether there was one.
    pub fn task_ended(&mut self) -> bool {
        self.task.take().is_some()
    }

    /// A debug session started, paused, ran on or ended: whether that's a
    /// change. One that ends takes the sessions started from it along.
    pub fn debug(&mut self, what: Debug, now: u64) -> bool {
        match what {
            Debug::Started { id, name, language, parent } => {
                self.sessions.retain(|s| s.id != id);
                self.sessions.push(Session { id, name, language, parent, paused: None, since: now });
                true
            }
            Debug::Paused(id, why) => self.sessions.iter_mut().find(|s| s.id == id).is_some_and(|s| s.paused.replace(why.clone()) != Some(why)),
            Debug::Resumed(id) => self.sessions.iter_mut().find(|s| s.id == id).is_some_and(|s| s.paused.take().is_some()),
            Debug::Ended(id) => {
                let before = self.sessions.len();
                self.sessions.retain(|s| s.id != id && s.parent != Some(id));
                self.sessions.len() != before
            }
        }
    }

    /// The session shown: the first one you started (not one an adapter
    /// asked for), and whether any is paused, and why.
    fn debugging(&self) -> Option<(&Session, Option<&str>)> {
        let shown = self.sessions.iter().find(|s| s.parent.is_none()).or(self.sessions.first())?;
        Some((shown, self.sessions.iter().find_map(|s| s.paused.as_deref())))
    }

    /// Whether you've gone idle now, after `minutes` without a key (0:
    /// never).
    pub fn check_idle(&mut self, now: u64, minutes: u32) -> bool {
        let gone = minutes > 0 && now.saturating_sub(self.input) >= u64::from(minutes) * 60_000;
        if gone && !self.idle {
            self.idle = true;
            return true;
        }
        false
    }
}

/// Discord's limit on each text: 128 characters, and at least 2.
fn fit(text: &str) -> String {
    let mut out: String = match text.chars().count() {
        n if n > 128 => text.chars().take(127).chain(['\u{2026}']).collect(),
        _ => text.to_string(),
    };
    while out.chars().count() < 2 {
        out.push(' ');
    }
    out
}

/// The activity Discord shows for `editor`, kept to `privacy`; none when
/// nothing's to be shown (a private project).
pub fn activity(editor: &Editor, privacy: &Privacy) -> Option<Value> {
    if editor.project.as_ref().is_some_and(|p| privacy.is_private(p)) {
        return None;
    }
    let mut activity = Map::new();
    let mut assets = Map::new();
    let file = editor.file.as_deref().map(|path| (path.rsplit(['/', '\\']).next().unwrap_or(path), language(path)));
    let debugging = editor.debugging();
    let task = editor.task.as_ref();
    let details = match (file, debugging, task) {
        _ if editor.idle => "Idle".to_string(),
        (_, Some((s, _)), _) if privacy.hide_files => language_named(&s.language).map_or("Debugging".to_string(), |lang| format!("Debugging {lang}")),
        (_, Some((s, _)), _) => format!("Debugging {}", s.name.rsplit(['/', '\\']).next().unwrap_or(&s.name)),
        (_, _, Some(_)) if privacy.hide_files => "Running a task".to_string(),
        (_, _, Some((name, _))) => format!("Running {name}"),
        (Some((_, (_, Some(lang)))), ..) if privacy.hide_files => format!("Editing {lang}"),
        (Some(_), ..) if privacy.hide_files => "Editing a file".to_string(),
        (Some((name, _)), ..) => format!("Editing {name}"),
        (None, ..) => "Starting".to_string(),
    };
    activity.insert("details".into(), fit(&details).into());
    if let Some(project) = editor.project.as_ref().filter(|_| !privacy.hide_projects) {
        activity.insert("state".into(), fit(&format!("In {}", project.name)).into());
    }
    let small = match (debugging, task) {
        _ if editor.idle => Some((IDLE, "Idle".to_string())),
        (Some((_, Some(why))), _) if !why.is_empty() => Some((DEBUG, format!("Paused ({why})"))),
        (Some((_, Some(_))), _) => Some((DEBUG, "Paused".to_string())),
        (Some(_), _) => Some((DEBUG, "Debugging".to_string())),
        (None, Some(_)) => Some((TASK, "Running a task".to_string())),
        (None, None) => None,
    };
    match file {
        Some((_, (key, lang))) => {
            assets.insert("large_image".into(), key.into());
            assets.insert("large_text".into(), fit(lang.unwrap_or("Text")).into());
            let (image, text) = small.unwrap_or((FENIX, "Fenix".to_string()));
            assets.insert("small_image".into(), image.into());
            assets.insert("small_text".into(), fit(&text).into());
        }
        None => {
            assets.insert("large_image".into(), FENIX.into());
            assets.insert("large_text".into(), "Fenix".into());
            if let Some((image, text)) = small.filter(|_| !editor.idle) {
                assets.insert("small_image".into(), image.into());
                assets.insert("small_text".into(), fit(&text).into());
            }
        }
    }
    activity.insert("assets".into(), Value::Object(assets));
    // Debugging, or a task running: since it started.
    let doing = debugging.map(|(s, _)| s.since).or(task.map(|(_, at)| *at));
    let since = match privacy.elapsed {
        _ if editor.idle => None,
        Elapsed::None => None,
        Elapsed::Project | Elapsed::File if doing.is_some() => doing,
        Elapsed::Project => Some(editor.project_since),
        Elapsed::File => Some(editor.file_since),
    };
    if let Some(start) = since {
        activity.insert("timestamps".into(), json!({ "start": start }));
    }
    activity.insert("instance".into(), false.into());
    Some(Value::Object(activity))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str) -> Option<Project> {
        Some(Project { name: name.into(), root: format!("/home/me/src/{name}") })
    }

    fn editing(file: &str, in_project: &str) -> Editor {
        let mut e = Editor::new(1_000);
        e.moved(file, project(in_project), 2_000);
        e
    }

    #[test]
    fn languages_by_extension_and_by_name() {
        assert_eq!(language("/p/src/main.rs"), ("rust", Some("Rust")));
        assert_eq!(language("C:\\p\\lib.HPP"), ("cpp", Some("C++")));
        assert_eq!(language("/p/Dockerfile"), ("docker", Some("Dockerfile")));
        assert_eq!(language("/p/CMakeLists.txt"), ("cmake", Some("CMake")));
        assert_eq!(language("/p/notes.txt"), (TEXT, None));
        // A dotfile's name isn't its extension.
        assert_eq!(language("/p/.rs"), (TEXT, None));
        // Every key is one Discord takes: lowercase, no spaces.
        for (_, key, _) in LANGUAGES {
            assert!(key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()), "{key}");
        }
    }

    #[test]
    fn the_file_its_language_the_project_and_since_when() {
        let a = activity(&editing("/home/me/src/fenix/src/main.rs", "fenix"), &Privacy::default()).unwrap();
        assert_eq!(
            a,
            json!({
                "details": "Editing main.rs",
                "state": "In fenix",
                "assets": { "large_image": "rust", "large_text": "Rust", "small_image": "fenix", "small_text": "Fenix" },
                "timestamps": { "start": 2_000 },
                "instance": false,
            })
        );
        // Since the file, not the project.
        let mut e = editing("/home/me/src/fenix/src/main.rs", "fenix");
        e.moved("/home/me/src/fenix/src/lib.rs", project("fenix"), 9_000);
        assert_eq!(e.project_since, 2_000);
        let file = Privacy { elapsed: Elapsed::File, ..Privacy::default() };
        assert_eq!(activity(&e, &file).unwrap()["timestamps"]["start"], 9_000);
        let none = Privacy { elapsed: Elapsed::None, ..Privacy::default() };
        assert!(activity(&e, &none).unwrap().get("timestamps").is_none());
    }

    #[test]
    fn before_any_file_its_fenix() {
        let a = activity(&Editor::new(5), &Privacy::default()).unwrap();
        assert_eq!(a["details"], "Starting");
        assert_eq!(a["assets"], json!({ "large_image": "fenix", "large_text": "Fenix" }));
        assert!(a.get("state").is_none());
    }

    #[test]
    fn hidden_files_and_projects() {
        let e = editing("/home/me/src/secret/plans.py", "secret");
        let hide = Privacy { hide_files: true, hide_projects: true, ..Privacy::default() };
        let a = activity(&e, &hide).unwrap();
        assert_eq!(a["details"], "Editing Python");
        assert!(a.get("state").is_none());
        assert!(!a.to_string().contains("plans") && !a.to_string().contains("secret"));
        assert_eq!(activity(&editing("/x/notes.txt", "x"), &hide).unwrap()["details"], "Editing a file");
    }

    #[test]
    fn a_private_project_shows_nothing() {
        let private = Privacy { private: vec!["work-*".into(), "/home/me/src/client/".into()], ..Privacy::default() };
        assert!(activity(&editing("/home/me/src/work-api/a.rs", "work-api"), &private).is_none());
        assert!(activity(&editing("/home/me/src/client/a.rs", "client"), &private).is_none());
        assert!(activity(&editing("/home/me/src/WORK-ui/a.rs", "WORK-ui"), &private).is_none());
        assert!(activity(&editing("/home/me/src/fenix/a.rs", "fenix"), &private).is_some());
        // A file in no project isn't one of them.
        let mut e = Editor::new(0);
        e.moved("/tmp/a.rs", None, 0);
        assert!(activity(&e, &private).is_some());
    }

    #[test]
    fn idle_after_minutes_without_a_key() {
        let mut e = editing("/home/me/src/fenix/a.rs", "fenix");
        e.typed(10_000);
        assert!(!e.check_idle(10_000 + 4 * 60_000, 5));
        assert!(e.check_idle(10_000 + 5 * 60_000, 5));
        // Told once.
        assert!(!e.check_idle(10_000 + 6 * 60_000, 5));
        let a = activity(&e, &Privacy::default()).unwrap();
        assert_eq!(a["details"], "Idle");
        assert_eq!(a["assets"]["small_image"], IDLE);
        assert!(a.get("timestamps").is_none());
        // A key, and it's back, its time as it was.
        assert!(e.typed(20 * 60_000));
        assert!(!e.typed(20 * 60_000 + 1));
        assert_eq!(activity(&e, &Privacy::default()).unwrap()["timestamps"]["start"], 2_000);
        // 0: never idle.
        assert!(!e.check_idle(u64::MAX / 2, 0));
    }

    #[test]
    fn moving_is_a_change_only_when_it_is() {
        let mut e = editing("/home/me/src/fenix/a.rs", "fenix");
        assert!(!e.moved("/home/me/src/fenix/a.rs", project("fenix"), 50_000));
        assert!(e.moved("/home/me/src/pets/b.rs", project("pets"), 60_000));
        assert_eq!((e.project_since, e.file_since), (60_000, 60_000));
    }

    fn started(id: u64, name: &str, parent: Option<u64>) -> Debug {
        Debug::Started { id, name: name.into(), language: "python".into(), parent }
    }

    #[test]
    fn debugging_and_its_stops() {
        let mut e = editing("/home/me/src/fenix/app.py", "fenix");
        assert!(e.debug(started(1, "/home/me/src/fenix/app.py", None), 30_000));
        let a = activity(&e, &Privacy::default()).unwrap();
        assert_eq!(a["details"], "Debugging app.py");
        assert_eq!(a["state"], "In fenix");
        assert_eq!(a["assets"]["large_image"], "python");
        assert_eq!((a["assets"]["small_image"].as_str(), a["assets"]["small_text"].as_str()), (Some(DEBUG), Some("Debugging")));
        assert_eq!(a["timestamps"]["start"], 30_000, "since it started");
        // A child session an adapter asked for: its stop is shown, the
        // name is still the one you started.
        assert!(e.debug(started(2, "child", Some(1)), 31_000));
        assert!(e.debug(Debug::Paused(2, "breakpoint".into()), 32_000));
        assert!(!e.debug(Debug::Paused(2, "breakpoint".into()), 32_500), "the same stop");
        let a = activity(&e, &Privacy::default()).unwrap();
        assert_eq!(a["details"], "Debugging app.py");
        assert_eq!(a["assets"]["small_text"], "Paused (breakpoint)");
        assert!(e.debug(Debug::Resumed(2), 33_000));
        assert!(!e.debug(Debug::Resumed(2), 33_000));
        assert!(!e.debug(Debug::Paused(9, "step".into()), 0), "no such session");
        // Hidden file names: the language, not the program.
        let hide = Privacy { hide_files: true, ..Privacy::default() };
        assert_eq!(activity(&e, &hide).unwrap()["details"], "Debugging Python");
        // Its end takes the child along, and it's editing again.
        assert!(e.debug(Debug::Ended(1), 40_000));
        assert!(e.sessions.is_empty());
        assert_eq!(activity(&e, &Privacy::default()).unwrap()["details"], "Editing app.py");
        assert!(!e.debug(Debug::Ended(1), 40_000));
    }

    #[test]
    fn a_task_running() {
        let mut e = editing("/home/me/src/fenix/src/main.rs", "fenix");
        assert!(e.task_started("cargo build", 50_000));
        let a = activity(&e, &Privacy::default()).unwrap();
        assert_eq!(a["details"], "Running cargo build");
        assert_eq!((a["assets"]["small_image"].as_str(), a["assets"]["small_text"].as_str()), (Some(TASK), Some("Running a task")));
        assert_eq!(a["timestamps"]["start"], 50_000);
        let hide = Privacy { hide_files: true, elapsed: Elapsed::None, ..Privacy::default() };
        let a = activity(&e, &hide).unwrap();
        assert_eq!(a["details"], "Running a task");
        assert!(a.get("timestamps").is_none());
        // Debugging comes first; idle before both.
        e.debug(started(1, "tests", None), 51_000);
        assert_eq!(activity(&e, &Privacy::default()).unwrap()["details"], "Debugging tests");
        e.input = 0;
        e.check_idle(60 * 60_000, 5);
        assert_eq!(activity(&e, &Privacy::default()).unwrap()["details"], "Idle");
        e.typed(60 * 60_000);
        e.debug(Debug::Ended(1), 0);
        assert!(e.task_ended());
        assert!(!e.task_ended());
        assert_eq!(activity(&e, &Privacy::default()).unwrap()["details"], "Editing main.rs");
        assert_eq!(activity(&e, &Privacy::default()).unwrap()["timestamps"]["start"], 2_000);
    }

    #[test]
    fn languages_by_fenixs_names() {
        assert_eq!(language_named("cpp"), Some("C++"));
        assert_eq!(language_named("csharp"), Some("C#"));
        assert_eq!(language_named("Python"), Some("Python"));
        assert_eq!(language_named("brainfuck"), None);
    }

    #[test]
    fn text_is_kept_to_discords_limits() {
        let long = format!("/p/{}.rs", "x".repeat(300));
        let a = activity(&editing(&long, "p"), &Privacy::default()).unwrap();
        let details = a["details"].as_str().unwrap();
        assert_eq!(details.chars().count(), 128);
        assert!(details.ends_with('\u{2026}'));
        assert_eq!(fit("x"), "x ");
    }
}
