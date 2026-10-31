//! RuboCop, or Standard in a project that uses it (Standard is RuboCop
//! with its rules fixed): the formatter Fenix runs, and the linter on
//! save. Formatting fixes the layout only (`--fix-layout`): a formatter
//! that takes a variable it thinks useless out of a method half written
//! does more than format. The linter reads `--format json`, each problem
//! with RuboCop's correction for it where it gives one (1.92's
//! `correction`). The project's own, by `bundle exec`, when its
//! Gemfile.lock pins it; else the one installed.

use fenix_extension_api::{Command, Problem, Severity, TextEdit};
use fenix_lang_kit::lint::from_one;
use fenix_lang_kit::{Linter, Output};
use serde::Deserialize;

use crate::project::Project;
use crate::tools::Tools;

/// Ruby's files by their extensions, and by their names.
pub const EXTENSIONS: &[&str] = &["rb", "rbw", "rake", "gemspec", "ru", "jbuilder", "builder", "thor", "podspec"];
pub const NAMES: &[&str] = &["Gemfile", "gems.rb", "Rakefile", "Guardfile", "Capfile", "Vagrantfile", "Podfile", "Fastfile", "Appfile", "Dangerfile", "Berksfile", "Brewfile", "Steepfile", "Thorfile", ".irbrc", ".pryrc", ".simplecov"];

/// Which of the two styles the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    RuboCop,
    Standard,
}

impl Style {
    pub fn of(project: &Project) -> Style {
        if project.standard() {
            Style::Standard
        } else {
            Style::RuboCop
        }
    }

    /// Its gem, and its program.
    pub fn gem(self) -> &'static str {
        match self {
            Style::RuboCop => "rubocop",
            Style::Standard => "standard",
        }
    }

    pub fn program(self) -> &'static str {
        match self {
            Style::RuboCop => "rubocop",
            Style::Standard => "standardrb",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Style::RuboCop => "RuboCop",
            Style::Standard => "Standard",
        }
    }
}

/// The project's style and its command, without arguments; none when
/// there's none to run.
pub fn command(project: &Project, tools: &Tools) -> Option<(Style, Command)> {
    let style = Style::of(project);
    tools.gem_program(project, style.gem(), style.program()).map(|c| (style, c))
}

/// What Fenix formats Ruby with: the buffer on stdin, `${file}` its path
/// (for the project's config), the report on stderr and the text alone on
/// stdout. It exits 1 when what's left isn't layout; the text is kept.
pub fn formatter(cmd: &Command) -> Command {
    cmd.clone().args(["--fix-layout", "--format", "quiet", "--stderr", "--stdin", "${file}"])
}

/// The linter on save, for `style`'s command.
pub fn linter(style: Style, cmd: &Command) -> Linter {
    let cmd = cmd.clone();
    Linter::with_fixes(style.program(), EXTENSIONS, move |file| cmd.clone().args(["--format", "json", "--force-exclusion", file]), move |out, file| parse(out, file, style)).named(NAMES)
}

#[derive(Deserialize)]
struct Report {
    files: Vec<File>,
}

#[derive(Deserialize)]
struct File {
    path: String,
    offenses: Vec<Offense>,
}

#[derive(Deserialize)]
struct Offense {
    severity: String,
    message: String,
    cop_name: String,
    location: Location,
    #[serde(default)]
    correction: Option<Correction>,
}

#[derive(Deserialize)]
struct Location {
    start_line: u32,
    start_column: u32,
    last_line: u32,
    last_column: u32,
}

#[derive(Deserialize)]
struct Correction {
    safe: bool,
    edits: Vec<Edit>,
}

#[derive(Deserialize)]
struct Edit {
    start_line: u32,
    start_column: u32,
    last_line: u32,
    last_column: u32,
    begin_pos: u32,
    end_pos: u32,
    replacement: String,
}

impl Edit {
    /// As a text edit. Its columns are 1-based, its last one past its
    /// end, but 1 where the end is a line's start; so an edit over
    /// lines ending at column 1 can't be told from one ending a character
    /// in, and isn't offered.
    fn text_edit(self) -> Option<TextEdit> {
        let start = from_one(self.start_line, self.start_column);
        let end = if self.begin_pos == self.end_pos {
            start
        } else if self.start_line == self.last_line {
            (start.0, start.1 + (self.end_pos - self.begin_pos))
        } else if self.last_column > 1 {
            (self.last_line.saturating_sub(1), self.last_column)
        } else {
            return None;
        };
        Some(TextEdit::new(start, end, self.replacement))
    }
}

/// `fatal` and `error` are errors, `warning` (`Lint/`) warnings, a
/// convention or a refactoring a note.
fn severity(word: &str) -> Severity {
    match word {
        "fatal" | "error" => Severity::Error,
        "warning" => Severity::Warning,
        _ => Severity::Info,
    }
}

/// What `--format json` said of `file`: its offenses, each with its
/// correction ("(unsafe)" after one RuboCop makes only with `-A`). It
/// exits 1 when it found some, 2 when it couldn't run.
pub fn parse(out: &Output, file: &str, style: Style) -> Result<Vec<Problem>, String> {
    let Some(at) = out.stdout.find('{') else {
        return if out.ok() { Ok(Vec::new()) } else { Err(out.why()) };
    };
    let report: Report = fenix_lang_kit::serde_json::from_str(&out.stdout[at..]).map_err(|e| format!("{}: {e}: {}", style.program(), out.why()))?;
    let file = file.replace('\\', "/");
    let file = file.trim_start_matches("./");
    Ok(report
        .files
        .into_iter()
        .filter(|f| f.path.replace('\\', "/").ends_with(file))
        .flat_map(|f| f.offenses)
        .map(|o| {
            let l = &o.location;
            let message = o.message.lines().next().unwrap_or_default().to_string();
            let mut p = Problem::new(severity(&o.severity), from_one(l.start_line, l.start_column), (l.last_line.saturating_sub(1), l.last_column), message).source(style.program());
            p.code = Some(o.cop_name.clone());
            let mut p = p;
            if let Some(c) = o.correction.filter(|c| !c.edits.is_empty()) {
                let edits: Option<Vec<TextEdit>> = c.edits.into_iter().map(Edit::text_edit).collect();
                if let Some(edits) = edits {
                    let title = format!("Correct {}", o.cop_name);
                    p = p.fix(&if c.safe { title } else { format!("{title} (unsafe)") }, edits);
                }
            }
            p
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    const REPORT: &str = include_str!("samples/rubocop.json");

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: stderr.into() }
    }

    #[test]
    fn rubocops_offenses_are_problems_with_its_corrections() {
        let p = parse(&out(1, REPORT, ""), "lib/orbit.rb", Style::RuboCop).unwrap();
        let rows: Vec<_> = p.iter().map(|p| (p.start, p.end, p.severity, p.code.as_deref().unwrap())).collect();
        assert_eq!(
            rows,
            [
                ((0, 0), (0, 1), Severity::Info, "Style/FrozenStringLiteralComment"),
                ((0, 8), (0, 14), Severity::Info, "Style/StringLiterals"),
                ((1, 0), (1, 11), Severity::Info, "Style/Documentation"),
                ((5, 4), (5, 10), Severity::Warning, "Lint/UselessAssignment"),
                ((8, 14), (8, 16), Severity::Info, "Style/DefWithParentheses"),
                ((9, 9), (9, 25), Severity::Info, "Style/StringConcatenation"),
            ],
            "the other file's left out"
        );
        assert_eq!(p[1].message, "Prefer single-quoted strings when you don't need string interpolation or special symbols.");
        // The magic comment, put in at the top: an edit of nothing.
        let header = &p[0].fixes[0];
        assert_eq!(header.title, "Correct Style/FrozenStringLiteralComment (unsafe)");
        assert_eq!((header.edits[0].start, header.edits[0].end, header.edits[0].text.as_str()), ((0, 0), (0, 0), "# frozen_string_literal: true\n"));
        let quotes = &p[1].fixes[0];
        assert_eq!((quotes.title.as_str(), quotes.edits[0].start, quotes.edits[0].end, quotes.edits[0].text.as_str()), ("Correct Style/StringLiterals", (0, 8), (0, 14), "'json'"));
        assert!(p[2].fixes.is_empty(), "documentation is yours to write");

        let broken = parse(&out(1, REPORT, ""), "lib/broken.rb", Style::RuboCop).unwrap();
        assert_eq!((broken[0].severity, broken[0].message.as_str()), (Severity::Error, "unexpected token $end"), "the parser's note left off");
    }

    #[test]
    fn nothing_said_is_nothing_found_unless_it_failed() {
        assert!(parse(&out(0, "", ""), "a.rb", Style::RuboCop).unwrap().is_empty());
        assert_eq!(parse(&out(2, "", "Error: unrecognized cop or department Foo/Bar"), "a.rb", Style::RuboCop).unwrap_err(), "Error: unrecognized cop or department Foo/Bar");
        let clean = r#"{"metadata":{},"files":[{"path":"a.rb","offenses":[]}],"summary":{}}"#;
        assert!(parse(&out(0, clean, ""), "a.rb", Style::Standard).unwrap().is_empty());
        // Over two lines, ending where the second starts: which character
        // ends it can't be told, so it isn't offered.
        let edit = Edit { start_line: 3, start_column: 5, last_line: 4, last_column: 1, begin_pos: 30, end_pos: 41, replacement: String::new() };
        assert!(edit.text_edit().is_none());
        let edit = Edit { start_line: 3, start_column: 5, last_line: 4, last_column: 7, begin_pos: 30, end_pos: 47, replacement: "x".into() };
        assert_eq!(edit.text_edit().map(|e| (e.start, e.end)), Some(((2, 4), (3, 7))));
    }

    #[test]
    fn the_projects_own_formats_and_lints() {
        let tools = Tools { bundle: Some("/r/bundle".into()), rubocop: Some("/g/rubocop".into()), standardrb: Some("/g/standardrb".into()), ..Tools::default() };
        let project = Project { gems: vec!["rubocop".into()], ..Project::default() };
        let (style, cmd) = command(&project, &tools).unwrap();
        assert_eq!(style, Style::RuboCop);
        assert_eq!(line(&formatter(&cmd)), "/r/bundle exec rubocop --fix-layout --format quiet --stderr --stdin ${file}");
        let l = linter(style, &cmd);
        assert_eq!(line(&(l.command)("lib/orbit.rb")), "/r/bundle exec rubocop --format json --force-exclusion lib/orbit.rb");
        assert!(l.takes("lib/tasks/db.rake") && l.takes("Gemfile") && l.takes("orbit.gemspec") && l.takes("config.ru") && !l.takes("app.py"));

        let standard = Project { standard_yml: true, ..Project::default() };
        let (style, cmd) = command(&standard, &tools).unwrap();
        assert_eq!((style, line(&cmd).as_str()), (Style::Standard, "/g/standardrb"));
        assert_eq!(linter(style, &cmd).name, "standardrb");
        assert_eq!(command(&Project::default(), &Tools::default()), None);
    }
}
