//! A saved Lua file linted: by selene (`--display-style=json2`, a JSON
//! object a line, its places from 0) or luacheck (`--formatter plain
//! --codes --ranges`, a line a warning, its places from 1). Both exit 1
//! when they found something, so their output is read whatever the exit
//! code; one that printed nothing it could read, and failed, says why.

use fenix_extension_api::{Command, Problem, Severity};
use fenix_lang_kit::lint::{from_one, severity};
use fenix_lang_kit::serde_json::{self, Value};
use fenix_lang_kit::{Linter, Output};

/// Which linter runs on save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintWith {
    Selene,
    Luacheck,
}

/// selene for a saved Lua file; it reads `selene.toml` in `/project`.
pub fn selene(selene: &Command) -> Linter {
    let selene = selene.clone();
    Linter::new("selene", &["lua"], move |file| selene.clone().args(["--display-style=json2", "--no-summary", file]), parse_selene)
}

/// luacheck for a saved Lua file; it reads `.luacheckrc` in `/project`.
pub fn luacheck(luacheck: &Command) -> Linter {
    let luacheck = luacheck.clone();
    Linter::new("luacheck", &["lua"], move |file| luacheck.clone().args(["--formatter", "plain", "--codes", "--ranges", "--no-color", file]), parse_luacheck)
}

/// What `selene --display-style=json2` said: each diagnostic, its lint
/// as its code, at its primary label. A bad config or a missing file is
/// said on stderr alone.
pub fn parse_selene(out: &Output, _file: &str) -> Result<Vec<Problem>, String> {
    let found: Vec<Value> = out.stdout.lines().filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok()).filter(|v| v["type"] == "Diagnostic").collect();
    if found.is_empty() && !out.ok() {
        return Err(format!("selene: {}", out.why()));
    }
    Ok(found
        .iter()
        .filter_map(|d| {
            let span = &d["primary_label"]["span"];
            let at = |k: &str| span.get(k).and_then(Value::as_u64).map(|n| n as u32);
            let (start, end) = ((at("start_line")?, at("start_column")?), (at("end_line")?, at("end_column")?));
            let sev = d["severity"].as_str().and_then(severity).unwrap_or(Severity::Warning);
            let mut p = Problem::new(sev, start, end, d["message"].as_str()?).source("selene");
            p.code = d["code"].as_str().map(str::to_string);
            Some(p)
        })
        .collect())
}

/// What `luacheck --formatter plain --codes --ranges` said:
/// `file:line:col-end: (W211) message`, the range's end the last column
/// of it. `E` codes are syntax errors; the rest warnings.
pub fn parse_luacheck(out: &Output, file: &str) -> Result<Vec<Problem>, String> {
    let file = file.replace('\\', "/");
    let found: Vec<Problem> = out
        .stdout
        .lines()
        .filter_map(|line| {
            let (rest, message) = line.split_once(": (")?;
            let (code, message) = message.split_once(") ")?;
            let mut parts = rest.rsplitn(3, ':');
            let (range, line_no, path) = (parts.next()?, parts.next()?.parse::<u32>().ok()?, parts.next()?);
            let path = path.replace('\\', "/");
            if !(path == file || path.ends_with(&format!("/{file}")) || file.ends_with(&format!("/{path}"))) {
                return None;
            }
            let (from, to) = range.split_once('-').unwrap_or((range, range));
            let (l, c) = from_one(line_no, from.parse().ok()?);
            let end = to.parse::<u32>().unwrap_or(c + 1).max(c + 1);
            let sev = if code.starts_with('E') { Severity::Error } else { Severity::Warning };
            let mut p = Problem::new(sev, (l, c), (l, end), message).source("luacheck");
            p.code = Some(code.to_string());
            Some(p)
        })
        .collect();
    // 0 clean, 1 warnings, 2 errors; 3 and up, it couldn't check.
    match out.code {
        Some(code) if code > 2 && found.is_empty() => Err(format!("luacheck: {}", out.why())),
        _ => Ok(found),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: stderr.into() }
    }

    /// A problem's severity, start, end and code.
    type Row = (Severity, (u32, u32), (u32, u32), String);

    fn rows(problems: &[Problem]) -> Vec<Row> {
        problems.iter().map(|p| (p.severity, p.start, p.end, p.code.clone().unwrap_or_default())).collect()
    }

    #[test]
    fn selenes_diagnostics_are_problems() {
        let p = parse_selene(&out(1, include_str!("samples/selene-json2.txt"), ""), "bad.lua").unwrap();
        assert_eq!(
            rows(&p),
            [
                (Severity::Warning, (1, 15), (1, 16), "unused_variable".into()),
                (Severity::Warning, (1, 17), (1, 18), "unused_variable".into()),
                (Severity::Error, (2, 9), (2, 25), "undefined_variable".into()),
                (Severity::Warning, (4, 15), (4, 28), "multiple_statements".into()),
                (Severity::Warning, (5, 6), (5, 7), "unused_variable".into()),
                (Severity::Error, (5, 18), (5, 23), "duplicate_keys".into()),
            ]
        );
        assert_eq!(p[2].message, "`undefined_global` is not defined");
        assert_eq!(parse_selene(&out(0, "", ""), "a.lua").unwrap(), Vec::new());
        let parse_error = r#"{"type":"Diagnostic","severity":"Error","code":"parse_error","message":"unexpected token `=`","primary_label":{"filename":"broken.lua","span":{"start":8,"start_line":0,"start_column":8,"end":9,"end_line":0,"end_column":9},"message":"expected an expression"},"notes":[],"secondary_labels":[]}"#;
        assert_eq!(rows(&parse_selene(&out(1, parse_error, ""), "broken.lua").unwrap()), [(Severity::Error, (0, 8), (0, 9), "parse_error".into())]);
        let bad = parse_selene(&out(1, "", "ERROR: Config file not in correct format: TOML parse error at line 1, column 7"), "a.lua").unwrap_err();
        assert!(bad.starts_with("selene: ") && bad.contains("TOML parse error"), "{bad}");
    }

    #[test]
    fn luachecks_warnings_are_problems() {
        let p = parse_luacheck(&out(2, include_str!("samples/luacheck-plain.txt"), ""), "bad.lua").unwrap();
        assert_eq!(
            rows(&p),
            [
                (Severity::Warning, (1, 15), (1, 16), "W211".into()),
                (Severity::Warning, (1, 17), (1, 18), "W212".into()),
                (Severity::Warning, (2, 9), (2, 25), "W113".into()),
                (Severity::Warning, (5, 6), (5, 7), "W211".into()),
                (Severity::Warning, (5, 11), (5, 12), "W314".into()),
            ],
            "broken.lua's are another file's"
        );
        assert_eq!(p[2].message, "accessing undefined variable 'undefined_global'");
        let broken = parse_luacheck(&out(2, include_str!("samples/luacheck-plain.txt"), ""), "src/broken.lua").unwrap();
        assert_eq!(rows(&broken), [(Severity::Error, (1, 0), (1, 1), "E011".into())]);
        assert_eq!(parse_luacheck(&out(0, "", ""), "a.lua").unwrap(), Vec::new());
        let why = parse_luacheck(&out(4, "", "Critical error: in config loaded from .luacheckrc: invalid value of option 'std'"), "a.lua").unwrap_err();
        assert!(why.contains("invalid value of option 'std'"), "{why}");
    }

    #[test]
    fn each_runs_on_the_file_saved() {
        let l = selene(&Command::new("/t/selene"));
        assert_eq!(line(&(l.command)("lua/orbit/init.lua")), "/t/selene --display-style=json2 --no-summary lua/orbit/init.lua");
        assert!(l.takes("lua/orbit/init.lua") && !l.takes("orbit-1.0-1.rockspec"));
        let l = luacheck(&Command::new("/h/.luarocks/bin/luacheck"));
        assert_eq!(line(&(l.command)("src/orbit.lua")), "/h/.luarocks/bin/luacheck --formatter plain --codes --ranges --no-color src/orbit.lua");
    }
}
