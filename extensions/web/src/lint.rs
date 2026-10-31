//! ESLint: its language server beside the TypeScript server, given the
//! settings it asks for (`language-server-settings`, 0.8.6; Fenix pulls
//! its problems), or on save with `--format json`. Biome's server, for a
//! project that uses Biome. And the formatter Fenix runs: Prettier, or
//! Biome (`biome.json`). Each is the project's own, from `node_modules`,
//! run by `node` (no shell, no `.cmd`); else the one installed.

use fenix_extension_api::serde_json::{json, Value};
use fenix_extension_api::{Command, Problem, Severity};
use fenix_lang_kit::lint::from_one;
use fenix_lang_kit::{Linter, Output};
use serde::Deserialize;

use crate::project::{local_bin, Project};
use crate::tools::Tools;

/// What ESLint lints.
pub const EXTENSIONS: &[&str] = &["js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts"];

/// Fenix's names for what Prettier formats.
pub const PRETTIER_LANGUAGES: &[&str] = &["javascript", "typescript", "tsx", "css", "html", "json"];
/// And Biome (its HTML is experimental).
pub const BIOME_LANGUAGES: &[&str] = &["javascript", "typescript", "tsx", "css", "json"];

/// The project's ESLint, by `node`: `None` when it isn't installed.
pub fn eslint(project: &Project, tools: &Tools, root: &str) -> Option<Command> {
    if !project.eslint {
        return None;
    }
    local_bin(root, "eslint", "eslint").map(|bin| tools.node().arg(bin))
}

/// What ESLint's server is given when it asks (`workspace/configuration`,
/// section `""`): VS Code's settings for it, as it reads them -- it fails
/// on a null. Flat config when the project has an `eslint.config.*`; the
/// server runs in the project, so it needs no workspace folder.
pub fn eslint_settings(project: &Project, root: &str) -> Value {
    let flat = ["js", "mjs", "cjs", "ts", "mts", "cts"].iter().any(|e| std::path::Path::new(root).join(format!("eslint.config.{e}")).is_file());
    let manager = match project.manager.program() {
        "pnpm" => "pnpm",
        "yarn" => "yarn",
        _ => "npm",
    };
    json!({
        "validate": "on",
        "packageManager": manager,
        "useESLintClass": false,
        "useFlatConfig": flat,
        "experimental": { "useFlatConfig": false },
        "codeAction": {
            "disableRuleComment": { "enable": true, "location": "separateLine", "commentStyle": "line" },
            "showDocumentation": { "enable": true }
        },
        "codeActionOnSave": { "enable": false, "mode": "all" },
        "format": false,
        "quiet": false,
        "onIgnoredFiles": "off",
        "options": {},
        "rulesCustomizations": [],
        "run": "onType",
        "problems": { "shortenToSingleLine": false },
        "nodePath": null
    })
}

/// Biome's own, else the one installed, without a subcommand.
fn biome(tools: &Tools, root: &str) -> Option<Command> {
    local_bin(root, "@biomejs/biome", "biome").map(|bin| tools.node().arg(bin)).or_else(|| tools.biome.as_deref().map(Command::new))
}

/// Biome's language server (`biome lsp-proxy`), for a project that uses
/// Biome: its lints as you type.
pub fn biome_server(project: &Project, tools: &Tools, root: &str) -> Option<Command> {
    project.biome.then(|| biome(tools, root)).flatten().map(|b| b.arg("lsp-proxy"))
}

pub fn linter(eslint: &Command) -> Linter {
    let eslint = eslint.clone();
    Linter::new("eslint", EXTENSIONS, move |file| eslint.clone().args(["--format", "json", file]), parse)
}

#[derive(Deserialize)]
struct FileResult {
    #[serde(rename = "filePath")]
    file_path: String,
    messages: Vec<Message>,
}

#[derive(Deserialize)]
struct Message {
    #[serde(rename = "ruleId")]
    rule_id: Option<String>,
    severity: u8,
    message: String,
    #[serde(default)]
    line: u32,
    #[serde(default)]
    column: u32,
    #[serde(rename = "endLine")]
    end_line: Option<u32>,
    #[serde(rename = "endColumn")]
    end_column: Option<u32>,
}

/// `eslint --format json`'s problems in `file`: 2 an error, 1 a
/// warning, a parse error (no rule) an error, its warning that the file
/// is ignored left out (ESLint 8 has no `--no-warn-ignored`). It exits 1 when it found
/// errors, 2 when ESLint itself failed (no config).
pub fn parse(out: &Output, file: &str) -> Result<Vec<Problem>, String> {
    if out.stdout.trim().is_empty() {
        return if out.ok() { Ok(Vec::new()) } else { Err(out.why()) };
    }
    let results: Vec<FileResult> = out.json()?;
    let file = file.replace('\\', "/");
    Ok(results
        .into_iter()
        .filter(|r| r.file_path.replace('\\', "/").ends_with(&file))
        .flat_map(|r| r.messages)
        // A file the project ignores: ESLint says so, as a warning.
        .filter(|m| !(m.rule_id.is_none() && m.severity == 1 && m.message.starts_with("File ignored")))
        .map(|m| {
            let severity = if m.severity >= 2 { Severity::Error } else { Severity::Warning };
            let start = from_one(m.line.max(1), m.column.max(1));
            let end = match (m.end_line, m.end_column) {
                (Some(l), Some(c)) => from_one(l, c),
                _ => start,
            };
            let mut p = Problem::new(severity, start, end, m.message);
            p.code = m.rule_id;
            p
        })
        .collect())
}

/// The formatter, and the languages it's for: Biome's when the project
/// uses it, else Prettier's when it does (a dependency, a `.prettierrc`,
/// `package.json`'s `prettier`). The project's own; else the one
/// installed; else none, and Fenix's stays.
pub fn formatter(project: &Project, tools: &Tools, root: &str) -> Option<(&'static [&'static str], Command)> {
    if project.biome {
        return Some((BIOME_LANGUAGES, biome(tools, root)?.args(["format", "--stdin-file-path=${file}"])));
    }
    if project.prettier {
        let prettier = local_bin(root, "prettier", "prettier").map(|bin| tools.node().arg(bin)).or_else(|| tools.prettier.as_deref().map(Command::new))?;
        return Some((PRETTIER_LANGUAGES, prettier.args(["--stdin-filepath", "${file}"])));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::temp;
    use crate::tests::line;

    fn out(code: i32, stdout: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: String::new() }
    }

    #[test]
    fn eslints_json_is_its_problems_in_the_file() {
        let problems = parse(&out(1, include_str!("samples/eslint.json")), "a.js").unwrap();
        type Row<'a> = (Severity, (u32, u32), (u32, u32), Option<&'a str>);
        let rows: Vec<Row> = problems.iter().map(|p| (p.severity, p.start, p.end, p.code.as_deref())).collect();
        assert_eq!(rows, [(Severity::Error, (0, 6), (0, 12), Some("no-unused-vars")), (Severity::Error, (1, 12), (1, 19), Some("no-undef"))]);
        assert_eq!(problems[1].message, "'missing' is not defined.");
        let syntax = parse(&out(1, include_str!("samples/eslint.json")), "src.js").unwrap();
        assert_eq!(syntax.len(), 1, "src.js's own");
        assert_eq!((syntax[0].severity, syntax[0].code.as_deref()), (Severity::Error, None), "a parse error");
        assert!(parse(&out(0, ""), "a.js").unwrap().is_empty());
        assert!(parse(&out(2, ""), "a.js").is_err(), "ESLint failed");
    }

    fn parse_ok(o: &Output) -> Result<Vec<Problem>, String> {
        parse(o, "a.js")
    }

    #[test]
    fn biome_wins_then_prettier_each_the_projects_own_first() {
        let dir = temp("format");
        let root = dir.to_string_lossy().into_owned();
        let tools = Tools { node: Some("/n/node".into()), prettier: Some("/g/prettier".into()), ..Tools::default() };
        let prettier = Project { prettier: true, ..Project::default() };
        let (languages, cmd) = formatter(&prettier, &tools, &root).unwrap();
        assert_eq!((languages, line(&cmd).as_str()), (PRETTIER_LANGUAGES, "/g/prettier --stdin-filepath ${file}"), "the one installed");
        std::fs::create_dir_all(dir.join("node_modules/prettier/bin")).unwrap();
        std::fs::write(dir.join("node_modules/prettier/package.json"), r#"{"bin": "./bin/prettier.cjs"}"#).unwrap();
        std::fs::write(dir.join("node_modules/prettier/bin/prettier.cjs"), "").unwrap();
        assert_eq!(formatter(&prettier, &tools, &root).map(|(_, c)| line(&c)).as_deref(), Some("/n/node node_modules/prettier/bin/prettier.cjs --stdin-filepath ${file}"));
        let biome = Project { prettier: true, biome: true, ..Project::default() };
        assert_eq!(formatter(&biome, &tools, &root), None, "Biome's project, and no Biome");
        assert_eq!(formatter(&Project::default(), &tools, &root), None, "a project that uses neither isn't formatted");
        let with = Tools { biome: Some("/g/biome".into()), ..tools.clone() };
        assert_eq!(formatter(&biome, &with, &root).map(|(l, c)| (l, line(&c))), Some((BIOME_LANGUAGES, "/g/biome format --stdin-file-path=${file}".to_string())));

        let eslint_project = Project { eslint: true, ..Project::default() };
        assert_eq!(eslint(&eslint_project, &tools, &root), None, "not installed");
        std::fs::create_dir_all(dir.join("node_modules/eslint/bin")).unwrap();
        std::fs::write(dir.join("node_modules/eslint/package.json"), r#"{"bin": {"eslint": "./bin/eslint.js"}}"#).unwrap();
        std::fs::write(dir.join("node_modules/eslint/bin/eslint.js"), "").unwrap();
        let cmd = eslint(&eslint_project, &tools, &root).unwrap();
        let l = linter(&cmd);
        assert!(l.takes("src/App.tsx") && !l.takes("styles.css"));
        assert_eq!(line(&(l.command)("src/App.tsx")), "/n/node node_modules/eslint/bin/eslint.js --format json src/App.tsx");
        assert!(parse_ok(&out(0, "[]")).unwrap().is_empty());
        let ignored = r#"[{"filePath": "/p/a.js", "messages": [{"ruleId": null, "severity": 1, "message": "File ignored because of a matching ignore pattern. Use \"--no-ignore\" to disable file ignore settings or use \"--no-warn-ignored\" to suppress this warning."}]}]"#;
        assert!(parse_ok(&out(0, ignored)).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
