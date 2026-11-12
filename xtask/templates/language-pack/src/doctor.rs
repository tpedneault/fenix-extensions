//! The project doctor's checks (`SPC p h`): each program the pack needs,
//! found, asked its version, and held to what the project wants.

use fenix_lang_kit::Program;

/// The program the language needs, as `permissions.process` names it.
///
/// TODO(you): the real tool's name (`cargo`, `go`, `zig`). It's checked
/// for the pack's tasks, which run it.
pub const TOOL: &str = "example-tool";

/// The checks for a project. The tool is asked `--version`, and a missing
/// one is a warning (`optional`); drop `optional` to make it a failure.
///
/// TODO(you): what the tool takes to print its version
/// (`.version_args(["version"])`), the oldest you support
/// (`.at_least("1.2")`), and -- once you've worked out how to install it
/// safely -- a fix row on the doctor's page, which runs a task or
/// downloads a release checked by its SHA-256:
///
/// ```text
/// .fix("Install it", "{{folder}}.install", Fix::Task(command("example-tool install")))
/// ```
///
/// and its command, `{{folder}}.install`, in `extension.toml`. This
/// template installs nothing: it doesn't know what your tool is.
pub fn programs() -> Vec<Program> {
    vec![Program::new(TOOL, "Example tool").optional()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_extension_api::Health;
    use fenix_lang_kit::Output;

    fn said(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    #[test]
    fn the_tool_is_asked_its_version() {
        let p = programs();
        assert_eq!((p[0].name.as_str(), p[0].version_args.as_slice()), (TOOL, ["--version".to_string()].as_slice()));
        let check = p[0].evaluate(Some("/bin/example-tool"), Some(&said("example-tool 1.4.2\n")));
        assert_eq!(check.health, Health::Ok);
        assert!(check.detail.contains("1.4.2"), "{}", check.detail);
    }

    #[test]
    fn a_missing_tool_is_a_warning_and_a_silent_one_too() {
        let p = programs();
        assert_eq!(p[0].evaluate(None, None).health, Health::Warn);
        assert_eq!(p[0].evaluate(Some("/bin/example-tool"), Some(&said(""))).health, Health::Warn, "no version to read");
    }
}
