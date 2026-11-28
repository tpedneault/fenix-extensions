//! Kiln's language server in Fenix: whether this extension registers `kiln
//! lsp` for the project (`project::add_language_server`), for which
//! languages, and why not when it doesn't -- decided here, carried out by
//! `lib.rs`.
//!
//! # What decides it
//!
//! | The project ([`Usability`]) | `.fenix/tools.json` ([`Listed`]) | kiln's languages | Registered |
//! |---|---|---|---|
//! | not known yet (kiln looked for, first read) | -- | -- | as it was |
//! | not usable (no kiln, too old, no `kiln.toml`) | -- | -- | none, saying why |
//! | usable | lists kiln, either form | -- | none: Fenix runs that one |
//! | usable | missing, unreadable, malformed | not answered yet | as it was |
//! | usable | the same | `kiln lsp --languages` answered | those |
//! | usable | the same | refused (a kiln without it), or failed | Markdown only, saying why |
//!
//! "Usable" is kiln found, new enough, and a `kiln.toml` it reads (a first
//! read that failed on an item file still counts: kiln's server reports
//! that file's problems itself).
//!
//! # Why it asks kiln, and when
//!
//! Which code languages Kiln serves depends on the project (`[code] paths`
//! and what's in them), and only kiln knows: `kiln lsp --languages
//! --format json`, which computes what `kiln init --fenix` writes. Kiln
//! 1.0.0 doesn't have it, and asking it is safe: it refuses at argument
//! parsing, exit 2, `kiln: unexpected argument '--languages' found (see
//! `kiln --help`)`, before any server starts (checked with stdin left open;
//! the queue closes it anyway), so no version test is needed and a later
//! kiln is used as soon as it's there. A kiln that refuses gets Markdown --
//! what every kiln's server serves.
//!
//! It asks once the project becomes usable, and again on what can change
//! the answer: `kiln.toml` saved, `r` or `kiln.reload`, another kiln
//! (`kiln.path`). Not on every reload: the languages move only with
//! `[code] paths`, and a reload already runs three kilns. A new answer is
//! kept until the next, so asking again never takes the server away
//! meanwhile.
//!
//! # Not twice
//!
//! A project that ran `kiln init --fenix` lists kiln in its own
//! `.fenix/tools.json`, and Fenix runs that one; Fenix names this
//! extension's server apart from the project's, so registering ours too
//! would run two kilns on every file. So a tools.json that lists kiln --
//! as an additional server, or the older form as a language's own server
//! -- wins, and ours is taken away. It's read again after every save and on
//! `r`, so removing the entry brings ours back.
//!
//! # Restarts
//!
//! Fenix starts a server again each time it's set by name, so it's set only
//! when what it would be set to -- the languages, or the kiln program --
//! differs from what was set; a reload that changes neither sets nothing.
//!
//! What it deliberately doesn't do: call Fenix, read files or run kiln
//! (`lib.rs` does, and hands the results in); speak LSP (kiln does, to
//! Fenix directly); or write `.fenix/tools.json` (that's `kiln init
//! --fenix`'s, and Fenix's).

use serde::Deserialize;

use crate::kiln::{Failure, Ran};
use crate::model::{self, Version};
use crate::project::{QueryId, State};

/// The name it's registered under (Fenix keeps it apart from the
/// project's own servers).
pub const NAME: &str = "kiln";

/// What the server is started with, after the kiln program.
pub const ARGS: [&str; 1] = ["lsp"];

/// What's asked of kiln for the languages.
pub const ASK: [&str; 4] = ["lsp", "--languages", "--format", "json"];

/// What every kiln's server serves, and all of it for a kiln that can't
/// say more.
pub const MARKDOWN: &str = "markdown";

/// The most of `tools.json` read: it's a few hundred bytes; past this it's
/// taken as not listing kiln, said as unreadable.
pub const MAX_TOOLS_JSON: u64 = 1024 * 1024;

/// Whether the project is one kiln's server is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Usability {
    /// Not known yet: kiln is being looked for, or the project read for the
    /// first time. Nothing changes until it is.
    Unknown,
    /// No: the line says why (kiln's own, or how to give a kiln).
    Unusable(String),
    Usable,
}

/// The project's state as [`Usability`].
pub fn usability(state: &State) -> Usability {
    match state {
        State::Idle | State::Probing | State::Loading => Usability::Unknown,
        State::NotFound(why) | State::NotKiln(why) | State::NoProject(why) => {
            Usability::Unusable(why.to_string())
        }
        State::TooOld(v) => Usability::Unusable(format!(
            "kiln {v} is older than {}, the oldest this extension reads: update Kiln",
            model::MIN_VERSION
        )),
        State::Loaded { .. } | State::Failed(_) => Usability::Usable,
    }
}

/// How `.fenix/tools.json` runs kiln's server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Form {
    /// In `lsp.additional`, beside each language's own server: what `kiln
    /// init --fenix` writes.
    Additional,
    /// As a language's own server (`lsp.markdown`): the form `kiln init
    /// --fenix` wrote before Fenix had additional servers, which it moves
    /// over when run again.
    Primary(String),
}

/// What `.fenix/tools.json` says about kiln.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listed {
    /// Missing, or it names no kiln.
    No,
    /// It couldn't be read, or isn't JSON Fenix would take: the line says
    /// why. Taken as not listing kiln -- Fenix reports the file itself, and
    /// runs nothing from it.
    Unreadable(String),
    Yes(Form),
}

/// What `.fenix/tools.json` says, from what reading it gave: its text,
/// `None` when it's missing, or why it couldn't be read.
pub fn listed(read: Result<Option<String>, String>) -> Listed {
    let text = match read {
        Ok(None) => return Listed::No,
        Ok(Some(text)) => text,
        Err(why) => return Listed::Unreadable(format!(".fenix/tools.json can't be read: {why}")),
    };
    // A byte-order mark, as Windows editors write one.
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return Listed::Unreadable(format!(".fenix/tools.json isn't valid JSON: {e}")),
    };
    let Some(lsp) = value.get("lsp").and_then(|l| l.as_object()) else {
        return Listed::No;
    };
    let additional = lsp
        .get("additional")
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten();
    for server in additional {
        // kiln's own test (`init --fenix` finds its entry by name), or any
        // entry that runs `kiln lsp` under another name.
        let named = server.get("name").and_then(|n| n.as_str()) == Some(NAME);
        if named || runs_kiln_lsp(server) {
            return Listed::Yes(Form::Additional);
        }
    }
    for (language, server) in lsp {
        if language == "additional" {
            continue;
        }
        // The older form is `"markdown": {"executable": "kiln", …}`, which
        // kiln matches by its executable alone.
        let older = language == MARKDOWN && executable(server).is_some_and(is_kiln);
        if older || runs_kiln_lsp(server) {
            return Listed::Yes(Form::Primary(language.clone()));
        }
    }
    Listed::No
}

fn executable(server: &serde_json::Value) -> Option<&str> {
    server.get("executable").and_then(|e| e.as_str())
}

/// A server entry whose program is kiln and whose arguments have `lsp`.
fn runs_kiln_lsp(server: &serde_json::Value) -> bool {
    let lsp = server
        .get("args")
        .and_then(|a| a.as_array())
        .is_some_and(|a| a.iter().any(|a| a.as_str() == Some("lsp")));
    lsp && executable(server).is_some_and(is_kiln)
}

/// `kiln`, `kiln.exe`, or a path ending in one (either slash).
fn is_kiln(executable: &str) -> bool {
    let file = executable
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default();
    file.eq_ignore_ascii_case("kiln") || file.eq_ignore_ascii_case("kiln.exe")
}

/// The languages kiln's server serves here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Languages {
    /// What `kiln lsp --languages` said.
    Served(Vec<String>),
    /// Markdown alone: kiln couldn't say more, and the line says why.
    MarkdownOnly(String),
}

impl Languages {
    pub fn list(&self) -> Vec<String> {
        match self {
            Languages::Served(l) => l.clone(),
            Languages::MarkdownOnly(_) => vec![MARKDOWN.to_string()],
        }
    }

    /// Why it's Markdown alone, when it is.
    pub fn note(&self) -> Option<&str> {
        match self {
            Languages::Served(_) => None,
            Languages::MarkdownOnly(why) => Some(why),
        }
    }
}

#[derive(Deserialize)]
struct Served {
    languages: Vec<String>,
}

/// The languages, from `kiln lsp --languages --format json`'s answer
/// (`version` is the kiln that gave it, for what's said); `None` when kiln
/// wasn't run (no kiln, another chosen meanwhile), so there's nothing to
/// go on yet.
///
/// The answer's `command` and `args` are not used: the server is the kiln
/// this extension runs (`kiln.path`, else the one found), which is the
/// kiln that answered; a `kiln` from the answer could be another one on
/// the `PATH`.
pub fn languages(result: &Result<Ran, Failure>, version: Option<Version>) -> Option<Languages> {
    let kiln = version.map_or_else(|| "This kiln".to_string(), |v| format!("kiln {v}"));
    let ran = match result {
        Err(Failure::Unusable(_)) => return None,
        Err(f) => return Some(failed(&f.line())),
        Ok(ran) => ran,
    };
    let text = match ran.expect(&[0]) {
        Ok(text) => text,
        Err(f) if f.code() == Some(2) && f.line().contains("unexpected argument '--") => {
            return Some(Languages::MarkdownOnly(format!(
                "{kiln} has no `kiln lsp --languages`, so its language server is added for Markdown only: a later kiln says which code languages it serves here too (or `kiln init --fenix` lists them in .fenix/tools.json, and Fenix runs that one instead)"
            )));
        }
        Err(f) => return Some(failed(&f.line())),
    };
    let read =
        model::object::<Served>("kiln lsp --languages", text).and_then(|s| clean(s.languages));
    Some(match read {
        Ok(languages) => Languages::Served(languages),
        Err(why) => failed(&why),
    })
}

fn failed(why: &str) -> Languages {
    Languages::MarkdownOnly(format!(
        "kiln couldn't say which languages its language server serves ({why}), so it's added for Markdown only until it can: r asks again"
    ))
}

/// The languages as Fenix names them: lowercase words, each once, kiln's
/// order kept, Markdown always among them. An empty list or a name that
/// isn't one is refused, not guessed at.
fn clean(languages: Vec<String>) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for l in languages {
        let l = l.trim().to_string();
        let fine = !l.is_empty()
            && l.len() <= 40
            && l.chars().all(|c| {
                c.is_ascii_lowercase()
                    || c.is_ascii_digit()
                    || matches!(c, '-' | '_' | '+' | '#' | '.')
            });
        if !fine {
            return Err(format!(
                "kiln lsp --languages: {:?} isn't a language's name",
                model::clip(&l, 40)
            ));
        }
        if !out.contains(&l) {
            out.push(l);
        }
    }
    if out.is_empty() {
        return Err("kiln lsp --languages named no language".into());
    }
    if !out.iter().any(|l| l == MARKDOWN) {
        out.insert(0, MARKDOWN.to_string());
    }
    Ok(out)
}

/// The server as registered: its languages and the kiln program it runs
/// ([`ARGS`] after it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    pub languages: Vec<String>,
    pub program: String,
}

/// Which holds now: for the doctor and the output page.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Status {
    /// Not decided yet: kiln is being looked for, the project read, or its
    /// languages asked.
    #[default]
    Pending,
    /// This extension registered it; `note` says why it's Markdown alone,
    /// when it is.
    Registered {
        server: Server,
        note: Option<String>,
    },
    /// The project's `.fenix/tools.json` runs it, so this extension doesn't.
    ToolsJson(Form),
    /// Not registered, and why (kiln missing, too old, no `kiln.toml`, or
    /// Fenix refusing it).
    NotRegistered(String),
}

/// What `lib.rs` does for it, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// `project::add_language_server(NAME, languages, Some(program lsp))`,
    /// then [`Lsp::refused`] if Fenix says no.
    Register(Server),
    /// `project::add_language_server(NAME, languages, None)`.
    Unregister(Vec<String>),
    /// A line for the output page (`info!`).
    Say(String),
}

/// What [`Lsp::step`] wants done.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Step {
    /// Ask kiln for the languages ([`ASK`] through `Project::query`), and
    /// hand its id to [`Lsp::asked`].
    pub ask: bool,
    pub actions: Vec<Action>,
}

/// What Fenix was last told.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Told {
    /// Nothing yet, by this run of the extension: Fenix may still hold a
    /// server from an earlier one, so the first "none" is said too.
    #[default]
    Never,
    Nothing,
    Server(Server),
    /// It refused this one: not tried again until what's wanted changes.
    Refused(Server, String),
}

#[derive(Debug, Default)]
pub struct Lsp {
    /// Ask kiln for the languages once the project is usable.
    ask: bool,
    asking: Option<QueryId>,
    languages: Option<Languages>,
    /// `None`: to be read before the next decision.
    tools: Option<Listed>,
    was_usable: bool,
    told: Told,
    status: Status,
    /// What was last said about tools.json, and about the languages, so
    /// each is said once while it holds.
    said_tools: Option<String>,
    said_note: Option<String>,
}

impl Lsp {
    /// Which holds now.
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// Whether `.fenix/tools.json` should be read and handed in
    /// ([`Lsp::tools`]) before the next [`Lsp::step`].
    pub fn wants_tools(&self) -> bool {
        self.tools.is_none()
    }

    /// What `.fenix/tools.json` says now.
    pub fn tools(&mut self, listed: Listed) {
        self.tools = Some(listed);
    }

    /// A file was saved: `tools.json` is read again before the next step,
    /// and the languages asked again when it was `kiln.toml` (`[code]
    /// paths` may have changed).
    pub fn saved(&mut self, path: &str) {
        self.tools = None;
        if file_is(path, "kiln.toml") {
            self.ask = true;
        }
    }

    /// `r`, `kiln.reload`: everything looked at again.
    pub fn again(&mut self) {
        self.tools = None;
        self.ask = true;
    }

    /// Another kiln (`kiln.path`): its languages are its own, so the last
    /// answer goes; the server stays as it is until the new kiln answers,
    /// then is set once with both changes.
    pub fn kiln_changed(&mut self) {
        self.again();
        self.languages = None;
        self.asking = None;
    }

    /// The languages were asked, as query `id`.
    pub fn asked(&mut self, id: QueryId) {
        self.asking = Some(id);
    }

    /// Query `id`'s answer. One that isn't the latest asked is dropped.
    pub fn answered(
        &mut self,
        id: QueryId,
        result: &Result<Ran, Failure>,
        version: Option<Version>,
    ) {
        if self.asking != Some(id) {
            return;
        }
        self.asking = None;
        if let Some(languages) = languages(result, version) {
            self.languages = Some(languages);
        }
    }

    /// Fenix refused to register `server`.
    pub fn refused(&mut self, server: Server, why: String) {
        self.status = Status::NotRegistered(format!("Fenix refused kiln's language server: {why}"));
        self.told = Told::Refused(server, why);
    }

    /// What to do now, given the project and the kiln program (`kiln.path`,
    /// else the one found, else why there's none).
    pub fn step(&mut self, usability: &Usability, program: Result<String, String>) -> Step {
        let mut step = Step::default();
        let usable = *usability == Usability::Usable;
        if usable && !self.was_usable {
            self.ask = true;
        }
        self.was_usable = usable;
        if usable && self.ask && self.asking.is_none() {
            self.ask = false;
            step.ask = true;
        }
        let want = match usability {
            Usability::Unknown => return step,
            Usability::Unusable(why) => Err(why.clone()),
            Usability::Usable => match self.tools.clone().unwrap_or(Listed::No) {
                Listed::Yes(form) => {
                    let line = in_tools_json(&form);
                    self.say_tools(&mut step, Some(line));
                    self.unregister(&mut step, "the project's .fenix/tools.json runs it");
                    self.status = Status::ToolsJson(form);
                    return step;
                }
                listed => {
                    let line = match listed {
                        Listed::Unreadable(why) => Some(format!(
                            "{why}: it isn't taken to run kiln's language server, so this extension adds it"
                        )),
                        _ => None,
                    };
                    self.say_tools(&mut step, line);
                    match (&self.languages, program) {
                        (None, _) => {
                            if !matches!(self.told, Told::Server(_)) {
                                self.status = Status::Pending;
                            }
                            return step;
                        }
                        (Some(_), Err(why)) => Err(why),
                        (Some(l), Ok(program)) => Ok((
                            Server {
                                languages: l.list(),
                                program,
                            },
                            l.note().map(str::to_string),
                        )),
                    }
                }
            },
        };
        match want {
            Err(why) => {
                self.unregister(&mut step, &why);
                self.status = Status::NotRegistered(why);
            }
            Ok((server, note)) => {
                let same = match &self.told {
                    Told::Server(s) | Told::Refused(s, _) => *s == server,
                    Told::Never | Told::Nothing => false,
                };
                if let Some(n) = &note {
                    if self.said_note.as_ref() != Some(n) {
                        step.actions.push(Action::Say(n.clone()));
                    }
                }
                self.said_note = note.clone();
                if !same {
                    step.actions.push(Action::Say(format!(
                        "kiln's language server added for {} ({} {})",
                        server.languages.join(", "),
                        server.program,
                        ARGS.join(" ")
                    )));
                    step.actions.push(Action::Register(server.clone()));
                    self.told = Told::Server(server.clone());
                    self.status = Status::Registered { server, note };
                } else if let Told::Server(_) = self.told {
                    self.status = Status::Registered { server, note };
                }
            }
        }
        step
    }

    fn say_tools(&mut self, step: &mut Step, line: Option<String>) {
        if let Some(l) = &line {
            if self.said_tools.as_ref() != Some(l) {
                step.actions.push(Action::Say(l.clone()));
            }
        }
        self.said_tools = line;
    }

    /// Takes ours away, if Fenix may hold one.
    fn unregister(&mut self, step: &mut Step, why: &str) {
        match std::mem::replace(&mut self.told, Told::Nothing) {
            Told::Server(s) => {
                step.actions.push(Action::Say(format!(
                    "kiln's language server taken away: {why}"
                )));
                step.actions.push(Action::Unregister(s.languages));
            }
            // Quietly: there's nothing to say about one this run never set.
            Told::Never => step.actions.push(Action::Unregister(Vec::new())),
            Told::Nothing | Told::Refused(..) => {}
        }
    }
}

fn in_tools_json(form: &Form) -> String {
    match form {
        Form::Additional => "the project's .fenix/tools.json runs kiln's language server (`lsp.additional`), so this extension adds none; Fenix runs that one once the project's programs are allowed (SPC p y)".to_string(),
        Form::Primary(language) => format!(
            "the project's .fenix/tools.json runs kiln as the {language} language server (`lsp.{language}`, the form kiln init --fenix wrote before Fenix had additional servers; run it again to move it), so this extension adds none"
        ),
    }
}

/// Whether `path` (full, either slash, or under `/project`) is a file
/// called `name`.
fn file_is(path: &str, name: &str) -> bool {
    path.rsplit(['/', '\\']).next() == Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::fixture;

    const V1: Version = Version {
        major: 1,
        minor: 0,
        patch: 0,
    };

    /// The new command's answer, written by hand (see
    /// tests/fixtures/hand/README).
    fn hand(path: &str) -> String {
        let at = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/hand/");
        std::fs::read_to_string(format!("{at}{path}")).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// A recorded answer: its stdout, stderr and exit code.
    fn recorded(name: &str) -> Result<Ran, Failure> {
        let code = fixture(&format!("{name}.exit")).trim().parse().unwrap();
        let at = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/out/");
        let stderr = std::fs::read_to_string(format!("{at}{name}.stderr")).unwrap_or_default();
        Ok(Ran {
            code: Some(code),
            stdout: fixture(&format!("{name}.json")),
            stderr: stderr.lines().map(str::to_string).collect(),
        })
    }

    fn ok(stdout: &str) -> Result<Ran, Failure> {
        Ok(Ran {
            code: Some(0),
            stdout: stdout.into(),
            stderr: vec![],
        })
    }

    fn served(l: &[&str]) -> Option<Languages> {
        Some(Languages::Served(l.iter().map(|s| s.to_string()).collect()))
    }

    #[test]
    fn the_new_commands_answer_is_read() {
        assert_eq!(
            languages(&ok(&hand("lsp-languages.json")), Some(V1)),
            served(&["markdown", "rust", "tcl"])
        );
        // Kiln's order kept, each once, Markdown put in when missing.
        assert_eq!(
            languages(
                &ok("{\"kiln_json\": 1, \"languages\": [\"rust\", \"rust\", \" tcl \"]}"),
                None
            ),
            served(&["markdown", "rust", "tcl"])
        );
    }

    #[test]
    fn kiln_1_0s_refusal_is_markdown_only() {
        // Recorded from kiln 1.0.0, which has no --languages: exit 2 at
        // argument parsing, before anything starts.
        let refused = recorded("demo/lsp-languages");
        assert_eq!(
            refused.as_ref().unwrap().stderr,
            ["kiln: unexpected argument '--languages' found (see `kiln --help`)"]
        );
        let l = languages(&refused, Some(V1)).unwrap();
        assert_eq!(l.list(), ["markdown"]);
        let note = l.note().unwrap();
        assert!(note.starts_with("kiln 1.0.0 has no `kiln lsp --languages`, so its language server is added for Markdown only: a later kiln"), "{note}");
        // The same from Kiln's own project.
        assert_eq!(languages(&recorded("own/lsp-languages"), Some(V1)), Some(l));
    }

    #[test]
    fn what_cant_be_read_is_markdown_only_saying_why() {
        for (answer, why) in [
            (
                ok("{\"kiln_json\": 2, \"languages\": [\"rust\"]}"),
                "kiln lsp --languages: its JSON is shape 2",
            ),
            (
                ok("{\"kiln_json\": 1, \"languages\": []}"),
                "named no language",
            ),
            (
                ok("{\"kiln_json\": 1, \"languages\": [\"Rust; rm\"]}"),
                "isn't a language's name",
            ),
            (ok("{\"kiln_json\": 1, \"langu"), "can't be read"),
            (Err(Failure::TimedOut(60)), "gave no answer in 60 s"),
            (
                Ok(Ran {
                    code: Some(2),
                    stdout: String::new(),
                    stderr: vec!["kiln: kiln.toml:3: unknown key `cod`".into()],
                }),
                "kiln.toml:3: unknown key",
            ),
        ] {
            let l = languages(&answer, Some(V1)).unwrap();
            assert_eq!(l.list(), ["markdown"]);
            let note = l.note().unwrap();
            assert!(
                note.contains(why) && note.ends_with("r asks again"),
                "{note}"
            );
        }
        // Not run at all: nothing to go on.
        assert_eq!(languages(&Err(Failure::Unusable("x".into())), None), None);
    }

    #[test]
    fn tools_json_in_either_form_lists_kiln() {
        let read = |t: &str| listed(Ok(Some(t.to_string())));
        // What kiln init --fenix writes now.
        assert_eq!(
            read(
                r#"{"lsp": {"rust": {"executable": "rust-analyzer"}, "additional": [{"name": "kiln", "executable": "kiln", "args": ["lsp"], "languages": ["markdown", "rust"]}]}}"#
            ),
            Listed::Yes(Form::Additional)
        );
        // Another name, kiln by its path (Windows).
        assert_eq!(
            read(
                r#"{"lsp": {"additional": [{"name": "spell", "executable": "spell-ls"}, {"name": "reqs", "executable": "C:\\Tools\\Kiln.exe", "args": ["lsp"], "languages": ["markdown"]}]}}"#
            ),
            Listed::Yes(Form::Additional)
        );
        // The older form, before Fenix had additional servers.
        assert_eq!(
            read("\u{feff}{\"lsp\": {\"markdown\": {\"executable\": \"kiln\", \"args\": [\"lsp\"]}}}"),
            Listed::Yes(Form::Primary("markdown".into()))
        );
        assert_eq!(
            read(r#"{"lsp": {"rust": {"executable": "/opt/kiln/kiln", "args": ["lsp"]}}}"#),
            Listed::Yes(Form::Primary("rust".into()))
        );
    }

    #[test]
    fn tools_json_without_kiln_doesnt() {
        let read = |t: &str| listed(Ok(Some(t.to_string())));
        for text in [
            "{}",
            r#"{"tasks": {}}"#,
            r#"{"lsp": []}"#,
            r#"{"lsp": {"rust": {"executable": "rust-analyzer"}}}"#,
            // kiln, but not its server.
            r#"{"lsp": {"additional": [{"name": "check", "executable": "kiln", "args": ["check"]}]}}"#,
            r#"{"lsp": {"additional": [{"name": "x", "executable": "kilnx", "args": ["lsp"]}]}}"#,
            r#"{"lsp": {"additional": "kiln"}}"#,
        ] {
            assert_eq!(read(text), Listed::No, "{text}");
        }
        assert_eq!(listed(Ok(None)), Listed::No);
        let Listed::Unreadable(why) = read(r#"{"lsp": {"#) else {
            panic!()
        };
        assert!(
            why.starts_with(".fenix/tools.json isn't valid JSON: "),
            "{why}"
        );
        assert_eq!(
            listed(Err("permission denied".into())),
            Listed::Unreadable(".fenix/tools.json can't be read: permission denied".into())
        );
    }

    fn usable() -> Usability {
        Usability::Usable
    }

    fn kiln() -> Result<String, String> {
        Ok("/usr/bin/kiln".into())
    }

    fn server(l: &[&str], program: &str) -> Server {
        Server {
            languages: l.iter().map(|s| s.to_string()).collect(),
            program: program.into(),
        }
    }

    fn registers(step: &Step) -> Vec<&Server> {
        step.actions
            .iter()
            .filter_map(|a| match a {
                Action::Register(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    /// An Lsp that has registered markdown and rust.
    fn registered() -> Lsp {
        let mut lsp = Lsp::default();
        lsp.tools(Listed::No);
        assert!(lsp.step(&usable(), kiln()).ask);
        lsp.asked(1);
        lsp.answered(1, &ok(&hand("lsp-languages.json")), Some(V1));
        let step = lsp.step(&usable(), kiln());
        assert_eq!(
            registers(&step),
            [&server(&["markdown", "rust", "tcl"], "/usr/bin/kiln")]
        );
        lsp
    }

    #[test]
    fn nothing_changes_until_the_project_is_known() {
        let mut lsp = Lsp::default();
        let step = lsp.step(&Usability::Unknown, kiln());
        assert_eq!(step, Step::default());
        assert_eq!(lsp.status(), &Status::Pending);
        assert!(lsp.wants_tools());
    }

    #[test]
    fn usable_asks_then_registers_what_kiln_said() {
        let lsp = registered();
        assert_eq!(
            lsp.status(),
            &Status::Registered {
                server: server(&["markdown", "rust", "tcl"], "/usr/bin/kiln"),
                note: None
            }
        );
    }

    #[test]
    fn plain_reloads_register_nothing_again() {
        let mut lsp = registered();
        for _ in 0..3 {
            lsp.saved("/home/me/p/design/tasks/TASK-001.md");
            lsp.tools(Listed::No);
            let step = lsp.step(&usable(), kiln());
            assert_eq!(step, Step::default());
        }
        // Asked again with the same answer: still nothing.
        lsp.again();
        lsp.tools(Listed::No);
        assert!(lsp.step(&usable(), kiln()).ask);
        lsp.asked(2);
        lsp.answered(2, &ok(&hand("lsp-languages.json")), Some(V1));
        assert_eq!(lsp.step(&usable(), kiln()), Step::default());
    }

    #[test]
    fn new_languages_register_exactly_once() {
        let mut lsp = registered();
        lsp.saved("C:\\p\\kiln.toml");
        lsp.tools(Listed::No);
        assert!(lsp.step(&usable(), kiln()).ask);
        lsp.asked(2);
        // Meanwhile the old server stays.
        assert_eq!(lsp.step(&usable(), kiln()), Step::default());
        lsp.answered(
            2,
            &ok("{\"kiln_json\": 1, \"languages\": [\"markdown\", \"python\"]}"),
            Some(V1),
        );
        let step = lsp.step(&usable(), kiln());
        assert_eq!(
            registers(&step),
            [&server(&["markdown", "python"], "/usr/bin/kiln")]
        );
        assert_eq!(lsp.step(&usable(), kiln()), Step::default());
    }

    #[test]
    fn an_old_answer_is_dropped() {
        let mut lsp = registered();
        lsp.again();
        lsp.tools(Listed::No);
        lsp.step(&usable(), kiln());
        lsp.asked(3);
        lsp.answered(
            2,
            &ok("{\"kiln_json\": 1, \"languages\": [\"python\"]}"),
            Some(V1),
        );
        assert_eq!(lsp.step(&usable(), kiln()), Step::default());
    }

    #[test]
    fn an_old_kiln_is_markdown_only_said_once() {
        let mut lsp = Lsp::default();
        lsp.tools(Listed::No);
        lsp.step(&usable(), kiln());
        lsp.asked(1);
        lsp.answered(1, &recorded("demo/lsp-languages"), Some(V1));
        let step = lsp.step(&usable(), kiln());
        let said: Vec<_> = step
            .actions
            .iter()
            .filter(|a| matches!(a, Action::Say(s) if s.starts_with("kiln 1.0.0 has no")))
            .collect();
        assert_eq!(said.len(), 1);
        assert_eq!(registers(&step), [&server(&["markdown"], "/usr/bin/kiln")]);
        // Asked again, refused again: not said again, not set again.
        lsp.again();
        lsp.tools(Listed::No);
        lsp.step(&usable(), kiln());
        lsp.asked(2);
        lsp.answered(2, &recorded("demo/lsp-languages"), Some(V1));
        assert_eq!(lsp.step(&usable(), kiln()), Step::default());
        assert!(matches!(
            lsp.status(),
            Status::Registered { note: Some(_), .. }
        ));
    }

    #[test]
    fn listed_in_tools_json_takes_ours_away_and_unlisted_brings_it_back() {
        let mut lsp = registered();
        lsp.saved("/p/.fenix/tools.json");
        assert!(lsp.wants_tools());
        lsp.tools(Listed::Yes(Form::Additional));
        let step = lsp.step(&usable(), kiln());
        assert_eq!(
            step.actions.last(),
            Some(&Action::Unregister(vec![
                "markdown".into(),
                "rust".into(),
                "tcl".into()
            ]))
        );
        assert!(
            matches!(&step.actions[0], Action::Say(s) if s.starts_with("the project's .fenix/tools.json runs kiln's language server"))
        );
        assert_eq!(lsp.status(), &Status::ToolsJson(Form::Additional));
        // Said once, taken away once.
        lsp.saved("/p/.fenix/tools.json");
        lsp.tools(Listed::Yes(Form::Additional));
        assert_eq!(lsp.step(&usable(), kiln()), Step::default());
        // The entry removed: ours again, the languages known already.
        lsp.saved("/p/.fenix/tools.json");
        lsp.tools(Listed::No);
        let step = lsp.step(&usable(), kiln());
        assert_eq!(
            registers(&step),
            [&server(&["markdown", "rust", "tcl"], "/usr/bin/kiln")]
        );
    }

    #[test]
    fn listed_from_the_start_registers_nothing() {
        let mut lsp = Lsp::default();
        lsp.tools(Listed::Yes(Form::Primary("markdown".into())));
        let step = lsp.step(&usable(), kiln());
        // Fenix told once that there's none of ours, in case an earlier
        // run left one; nothing said about a server never set.
        assert!(step.actions.contains(&Action::Unregister(vec![])));
        assert!(registers(&step).is_empty());
        assert!(
            matches!(&step.actions[0], Action::Say(s) if s.contains("as the markdown language server"))
        );
        assert_eq!(
            lsp.step(&usable(), kiln()),
            Step {
                ask: false,
                actions: vec![]
            }
        );
    }

    #[test]
    fn malformed_tools_json_is_said_once_and_ours_registered() {
        let mut lsp = Lsp::default();
        lsp.tools(listed(Ok(Some("{".into()))));
        let first = lsp.step(&usable(), kiln());
        assert!(
            matches!(&first.actions[0], Action::Say(s) if s.starts_with(".fenix/tools.json isn't valid JSON") && s.ends_with("so this extension adds it"))
        );
        lsp.asked(1);
        lsp.answered(1, &ok(&hand("lsp-languages.json")), Some(V1));
        lsp.tools(listed(Ok(Some("{".into()))));
        let step = lsp.step(&usable(), kiln());
        assert!(!step
            .actions
            .iter()
            .any(|a| matches!(a, Action::Say(s) if s.contains("valid JSON"))));
        assert_eq!(registers(&step).len(), 1);
    }

    #[test]
    fn kiln_lost_takes_it_away() {
        let why = "kiln isn't on the PATH: install Kiln, or set kiln.path to where kiln is";
        let mut lsp = registered();
        let step = lsp.step(&Usability::Unusable(why.into()), Err(why.into()));
        assert_eq!(
            step.actions,
            [
                Action::Say(format!("kiln's language server taken away: {why}")),
                Action::Unregister(vec!["markdown".into(), "rust".into(), "tcl".into()])
            ]
        );
        assert_eq!(lsp.status(), &Status::NotRegistered(why.into()));
        assert_eq!(
            lsp.step(&Usability::Unusable(why.into()), Err(why.into())),
            Step::default()
        );
        // Back: asked again, registered again.
        lsp.tools(Listed::No);
        assert!(lsp.step(&usable(), kiln()).ask);
    }

    #[test]
    fn another_kiln_is_set_once_with_its_languages() {
        let mut lsp = registered();
        lsp.kiln_changed();
        // The new kiln looked for: the old server stays meanwhile.
        assert_eq!(
            lsp.step(&Usability::Unknown, Ok("/opt/kiln".into())),
            Step::default()
        );
        assert!(lsp.wants_tools());
        lsp.tools(Listed::No);
        let step = lsp.step(&usable(), Ok("/opt/kiln".into()));
        assert!(step.ask && step.actions.is_empty());
        lsp.asked(5);
        lsp.answered(5, &ok(&hand("lsp-languages.json")), Some(V1));
        let step = lsp.step(&usable(), Ok("/opt/kiln".into()));
        assert_eq!(
            registers(&step),
            [&server(&["markdown", "rust", "tcl"], "/opt/kiln")]
        );
    }

    #[test]
    fn a_refusal_from_fenix_isnt_retried() {
        let mut lsp = Lsp::default();
        lsp.tools(Listed::No);
        lsp.step(&usable(), kiln());
        lsp.asked(1);
        lsp.answered(1, &ok(&hand("lsp-languages.json")), Some(V1));
        let step = lsp.step(&usable(), kiln());
        let s = registers(&step)[0].clone();
        lsp.refused(s, "kiln: not allowed".into());
        assert!(matches!(lsp.status(), Status::NotRegistered(w) if w.ends_with("not allowed")));
        assert_eq!(lsp.step(&usable(), kiln()), Step::default());
    }

    #[test]
    fn the_project_not_usable_says_why() {
        let line = "kiln: no kiln.toml in <dir> or any folder above it (run `kiln init` to start a project)";
        assert_eq!(
            usability(&State::NoProject(line)),
            Usability::Unusable(line.into())
        );
        assert_eq!(
            usability(&State::TooOld(Version {
                major: 0,
                minor: 9,
                patch: 0
            })),
            Usability::Unusable(
                "kiln 0.9.0 is older than 1.0.0, the oldest this extension reads: update Kiln"
                    .into()
            )
        );
        assert_eq!(usability(&State::Failed("kiln: x")), Usability::Usable);
        for s in [State::Idle, State::Probing, State::Loading] {
            assert_eq!(usability(&s), Usability::Unknown);
        }
        let mut lsp = Lsp::default();
        let step = lsp.step(&Usability::Unusable(line.into()), kiln());
        assert_eq!(step.actions, [Action::Unregister(vec![])]);
        assert!(!step.ask);
    }

    #[test]
    fn a_saved_file_is_named_either_way() {
        assert!(file_is("C:\\p\\kiln.toml", "kiln.toml"));
        assert!(file_is("/project/kiln.toml", "kiln.toml"));
        assert!(!file_is("/p/kiln.toml.bak", "kiln.toml"));
        assert!(
            is_kiln("kiln") && is_kiln("KILN.EXE") && is_kiln("/a/b/kiln") && !is_kiln("kiln-ls")
        );
    }
}
