//! `terraform validate -json` on a saved `.tf` file: what Terraform finds
//! without planning -- a syntax error, a reference to what isn't declared,
//! an argument a block doesn't take, a module not installed -- shown with
//! terraform-ls's problems. It checks the saved file's folder as one
//! module, so it speaks of every file there: each one's problems are set,
//! and a file that was clean is made clean.
//!
//! Its JSON (`format_version` 1.0) holds `diagnostics`, each a `severity`
//! (`error`, `warning`), a `summary`, a `detail` and, when it's about a
//! place, a `range` whose `filename` is relative to the folder
//! validated and whose `start` and `end` are a `line` and `column` from 1,
//! the end not included. One about the whole configuration (a provider
//! `terraform init` hasn't installed) has no `range`: it's shown at the top
//! of the file saved.

use fenix_extension_api::{Command, Problem, Severity};
use fenix_lang_kit::lint::from_one;
use fenix_lang_kit::serde_json::Value;
use fenix_lang_kit::{Linter, Output};

/// The folder of `file` (in the project, `modules/net/main.tf`): the module
/// Terraform validates. `.` at the top.
pub fn module_of(file: &str) -> String {
    let file = file.replace('\\', "/");
    match file.rsplit_once('/') {
        Some((dir, _)) if !dir.is_empty() => dir.to_string(),
        _ => ".".into(),
    }
}

/// `name` (as validate says it, relative to `dir`) as a path in the
/// project, `..` and `.` taken out.
pub fn in_project(dir: &str, name: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let name = name.replace('\\', "/");
    for part in dir.split('/').chain(name.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

/// `terraform validate -json` for the module a saved file is in.
pub fn validate(terraform: &Command) -> Linter {
    let terraform = terraform.clone();
    Linter::across(
        "terraform validate",
        &["tf"],
        move |file| {
            let dir = module_of(file);
            let cmd = if dir == "." { terraform.clone() } else { terraform.clone().arg(format!("-chdir={dir}")) };
            cmd.args(["validate", "-json", "-no-color"])
        },
        parse,
    )
}

fn position(point: &Value) -> Option<(u32, u32)> {
    let line = u32::try_from(point.get("line")?.as_u64()?).ok()?;
    let column = u32::try_from(point.get("column")?.as_u64()?).ok()?;
    Some(from_one(line, column))
}

/// What validate said, file by file, for the module `file` is in: an error
/// as an error and a warning as a warning, its summary and then its
/// detail as the message. It exits 1 when it found an error and 0 when it
/// found none, with its JSON either way; anything else printed (an old
/// Terraform, a folder that isn't a module) is the linter's own failure.
pub fn parse(out: &Output, file: &str) -> Result<Vec<(String, Vec<Problem>)>, String> {
    let Some(report) = out.json_stream().into_iter().next() else {
        return if out.ok() { Ok(Vec::new()) } else { Err(out.why()) };
    };
    let diagnostics = report.get("diagnostics").and_then(Value::as_array).ok_or_else(|| "its output isn't what validate -json prints".to_string())?;
    let dir = module_of(file);
    let mut files: Vec<(String, Vec<Problem>)> = Vec::new();
    for d in diagnostics {
        let severity = match d.get("severity").and_then(Value::as_str) {
            Some("warning") => Severity::Warning,
            _ => Severity::Error,
        };
        let summary = d.get("summary").and_then(Value::as_str).unwrap_or("").trim();
        let detail = d.get("detail").and_then(Value::as_str).unwrap_or("").trim();
        let message = if detail.is_empty() { summary.to_string() } else { format!("{summary}: {detail}") };
        let range = d.get("range");
        let place = range.and_then(|r| r.get("filename")).and_then(Value::as_str).map(|name| in_project(&dir, name));
        let start = range.and_then(|r| r.get("start")).and_then(position);
        let end = range.and_then(|r| r.get("end")).and_then(position);
        let (path, start, end) = match (place, start) {
            (Some(path), Some(start)) => {
                let end = end.filter(|e| *e > start).unwrap_or((start.0, start.1 + 1));
                (path, start, end)
            }
            // About the configuration, not a place in it: at the top of the file saved.
            _ => (file.replace('\\', "/"), (0, 0), (0, 1)),
        };
        let problem = Problem::new(severity, start, end, &message);
        match files.iter_mut().find(|(f, _)| *f == path) {
            Some((_, problems)) => problems.push(problem),
            None => files.push((path, vec![problem])),
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    fn out(code: i32, stdout: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: String::new() }
    }

    type Row<'a> = (&'a str, Severity, (u32, u32), (u32, u32), &'a str);
    type Owned = (String, Severity, (u32, u32), (u32, u32), String);

    fn rows(files: &[(String, Vec<Problem>)]) -> Vec<Owned> {
        files.iter().flat_map(|(f, ps)| ps.iter().map(move |p| (f.clone(), p.severity, p.start, p.end, p.message.clone()))).collect()
    }

    fn row(r: Row<'_>) -> Owned {
        (r.0.into(), r.1, r.2, r.3, r.4.into())
    }

    #[test]
    fn validate_runs_in_the_saved_files_module() {
        let l = validate(&Command::new("/t/terraform"));
        assert_eq!(line(&(l.command)("main.tf")), "/t/terraform validate -json -no-color");
        assert_eq!(line(&(l.command)("modules/net/main.tf")), "/t/terraform -chdir=modules/net validate -json -no-color");
        assert!(l.takes("main.tf") && l.takes("infra/vars.tf") && !l.takes("prod.tfvars") && !l.takes("main.hcl"));
        assert!(l.across, "it speaks of every file in the module");
        assert_eq!((module_of("main.tf"), module_of("a\\b\\main.tf")), (".".to_string(), "a/b".to_string()));
        assert_eq!(in_project(".", "main.tf"), "main.tf");
        assert_eq!(in_project("infra", "../modules/net/main.tf"), "modules/net/main.tf");
        assert_eq!(in_project("infra", "child\\main.tf"), "infra/child/main.tf");
    }

    #[test]
    fn errors_are_read_from_terraform_validates_json() {
        let files = parse(&out(1, include_str!("samples/validate-errors.json")), "main.tf").unwrap();
        assert_eq!(
            rows(&files),
            [
                row(("main.tf", Severity::Error, (17, 10), (17, 19), "Reference to undeclared local value: A local value with the name \"nme\" has not been declared. Did you mean \"name\"?")),
                row(("main.tf", Severity::Error, (21, 10), (21, 20), "Reference to undeclared input variable: An input variable with the name \"regoin\" has not been declared. Did you mean \"region\"?")),
            ],
            "from 0, the end after the last character: `local.nme` is columns 11 to 19 of line 18"
        );
    }

    #[test]
    fn a_module_below_the_project_has_its_files_named_from_the_project() {
        let files = parse(&out(1, include_str!("samples/validate-nested.json")), "modules/net/main.tf").unwrap();
        assert_eq!(rows(&files), [row(("modules/net/main.tf", Severity::Error, (6, 2), (6, 7), "Unsupported argument: An argument named \"bogus\" is not expected here."))]);
        let child = parse(&out(1, include_str!("samples/validate-child-error.json")), "main.tf").unwrap();
        assert_eq!(child.iter().map(|(f, ps)| (f.as_str(), ps.len())).collect::<Vec<_>>(), [("main.tf", 1), ("child/main.tf", 1)], "the module called, too");
        assert_eq!(child[0].1[0].severity, Severity::Warning);
        assert_eq!(child[1].1[0].message, "Reference to undeclared input variable: An input variable with the name \"nope\" has not been declared. This variable can be declared with a variable \"nope\" {} block.");
    }

    #[test]
    fn a_warning_is_a_warning_and_a_valid_configuration_has_none() {
        let files = parse(&out(0, include_str!("samples/validate-warning.json")), "main.tf").unwrap();
        assert_eq!(rows(&files), [row(("main.tf", Severity::Warning, (2, 11), (2, 14), "Deprecated variable got a value: Use new instead."))]);
        assert_eq!(parse(&out(0, include_str!("samples/validate-valid.json")), "main.tf").unwrap(), Vec::new());
        assert_eq!(parse(&out(0, ""), "main.tf").unwrap(), Vec::new());
    }

    #[test]
    fn a_syntax_error_is_where_terraform_says() {
        let files = parse(&out(1, include_str!("samples/validate-syntax.json")), "main.tf").unwrap();
        let r = rows(&files);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].2, r[0].3, r[0].4.as_str()), ((0, 16), (0, 17), "Unclosed configuration block: There is no closing brace for this block before the end of the file. This may be caused by incorrect brace nesting elsewhere in this file."));
        assert_eq!((r[1].2, r[1].4.as_str()), ((4, 0), "Unsupported block type: Blocks of type \"resource\" are not expected here."));
    }

    #[test]
    fn what_is_about_the_whole_configuration_is_at_the_top_of_the_file_saved() {
        let files = parse(&out(1, include_str!("samples/validate-missing-provider.json")), "infra/main.tf").unwrap();
        let r = rows(&files);
        assert_eq!((r.len(), r[0].0.as_str(), r[0].2, r[0].3, r[0].1), (1, "infra/main.tf", (0, 0), (0, 1), Severity::Error));
        assert!(r[0].4.starts_with("Missing required provider: This configuration requires provider registry.terraform.io/hashicorp/aws"), "{}", r[0].4);
        let module = parse(&out(1, include_str!("samples/validate-module-not-installed.json")), "main.tf").unwrap();
        assert_eq!(rows(&module)[0].2, (0, 0), "a module block: its first line");
        assert!(rows(&module)[0].4.starts_with("Module not installed: This module is not yet installed. Run \"terraform init\""));
    }

    #[test]
    fn what_isnt_validates_json_is_the_linters_failure() {
        let broken = Output { code: Some(1), stdout: String::new(), stderr: "Terraform has no command named \"validate\".\n".into() };
        assert_eq!(parse(&broken, "main.tf").unwrap_err(), "Terraform has no command named \"validate\".");
        assert!(parse(&out(1, "{\"valid\": false}"), "main.tf").is_err());
    }
}
