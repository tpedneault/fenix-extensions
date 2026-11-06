//! Debugging with earlybird (`ocamlearlybird debug`), OCaml's debug
//! adapter, on stdio. It debugs bytecode: an executable whose `dune` file
//! says `(modes byte exe)` has a `.bc` beside its `.exe`, and each such is
//! a launch of its own (`launches-set`), built by its task first and run
//! from `_build/default`. A native executable has no launch: earlybird
//! can't debug one.

use fenix_extension_api::{Command, LaunchConfig};
use fenix_lang_kit::serde_json::json;

use crate::project::Project;
use crate::tasks::bytecode_task;
use crate::tools::Tools;

/// Fenix's name for the language a launch is for.
pub const OCAML: &[&str] = &["ocaml"];

/// earlybird, when it's there: `ocamlearlybird debug`.
pub fn adapter(tools: &Tools) -> Option<Command> {
    tools.command("ocamlearlybird").map(|c| c.arg("debug"))
}

/// A launch for each bytecode executable. earlybird takes its arguments
/// as `arguments`, not `args`.
pub fn launches(project: &Project) -> Vec<LaunchConfig> {
    let attributes = json!({ "stopOnEntry": false, "arguments": [] });
    project
        .executables
        .iter()
        .filter(|e| e.byte)
        .map(|e| LaunchConfig::launch(&format!("OCaml: debug {}", e.label()), OCAML).program(&format!("${{project}}/_build/default/{}", e.path("bc"))).cwd("${project}").attributes(&attributes).pre_launch_task(&bytecode_task(e)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::dune_project;
    use crate::tests::line;

    #[test]
    fn each_bytecode_executable_is_a_launch() {
        let dir = dune_project("debug");
        let project = Project::read(&dir.to_string_lossy());
        let l = launches(&project);
        assert_eq!(l.len(), 1, "sim and replay are native only");
        assert_eq!((l[0].name.as_str(), l[0].program.as_deref(), l[0].pre_launch_task.as_deref()), ("OCaml: debug orbit", Some("${project}/_build/default/bin/main.bc"), Some("build bin/main.bc")));
        let attributes: fenix_lang_kit::serde_json::Value = fenix_lang_kit::serde_json::from_str(&l[0].attributes).unwrap();
        assert_eq!(attributes, json!({"stopOnEntry": false, "arguments": []}));
        let tools = Tools { ocamlearlybird: Some("/s/ocamlearlybird".into()), ..Tools::default() };
        assert_eq!(adapter(&tools).map(|c| line(&c)).as_deref(), Some("/s/ocamlearlybird debug"));
        assert!(adapter(&Tools::default()).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
