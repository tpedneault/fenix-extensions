//! The tasks (`SPC p t`): what you'd type in a terminal, each a name and
//! a command line.

use fenix_lang_kit::Tasks;

/// The pack's tasks.
///
/// TODO(you): the language's own -- its build, test, run and lint
/// commands. Each program they run is named in `extension.toml`'s
/// `permissions.process`. A pack that works the commands out from the
/// project (`zig build -l`'s steps) builds the table with `Tasks::add`.
pub fn tasks() -> Tasks {
    Tasks::table(&[("build", "example-tool build"), ("test", "example-tool test")])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_task_runs_the_tool_with_its_name() {
        let resolved = tasks().resolved();
        let lines: Vec<(String, String, Vec<String>)> = resolved.iter().map(|t| (t.name.clone(), t.cmd.program.clone(), t.cmd.args.clone())).collect();
        assert_eq!(lines, [("build".into(), "example-tool".into(), vec!["build".into()]), ("test".into(), "example-tool".into(), vec!["test".into()])]);
        assert!(resolved.iter().all(|t| t.cmd.program == crate::doctor::TOOL), "permissions.process names it");
    }
}
