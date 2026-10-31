//! The tasks (`SPC p t`), from Cargo.toml's targets as `cargo metadata`
//! gives them: build, run (one for each binary when there are several),
//! each example, test, bench when there are benches, clippy and fmt. In
//! a workspace each names its package (`-p`) and the workspace-wide ones
//! say `--workspace`. Until cargo has answered, the ones that need no
//! targets. A task of the same name in `.fenix/tools.json` wins.

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::project::{Metadata, Package, Target};
use crate::tools::Tools;

/// `cargo run`'s arguments for `target` of `package`: its package and
/// target named when there's a choice, its required features turned on.
fn run_args(m: &Metadata, package: &Package, target: &Target, flag: &str, one: bool) -> Vec<String> {
    let mut args = vec!["run".to_string()];
    if m.several() {
        args.extend(["-p".to_string(), package.name.clone()]);
    }
    if !one || flag == "--example" {
        args.extend([flag.to_string(), target.name.clone()]);
    }
    if !target.required_features.is_empty() {
        args.extend(["--features".to_string(), target.required_features.join(",")]);
    }
    args
}

/// The tasks, with `metadata` once cargo has said it.
pub fn tasks(metadata: Option<&Metadata>, workspace: bool, tools: &Tools) -> Tasks {
    let cargo = |args: &[&str]| tools.cargo().args(args.iter().copied());
    let all = workspace || metadata.is_some_and(Metadata::several);
    let wide = |cmd: Command, flag: &str| if all { cmd.arg(flag) } else { cmd };
    let mut t = Tasks::default();
    t.add("build", wide(cargo(&["build"]), "--workspace"));
    if let Some(m) = metadata {
        let bins = m.targets("bin");
        for (p, target) in &bins {
            let name = if bins.len() == 1 { "run".to_string() } else { format!("run {}", target.name) };
            t.add(&name, cargo(&[]).args(run_args(m, p, target, "--bin", bins.len() == 1)));
        }
        for (p, target) in m.targets("example") {
            t.add(&format!("example {}", target.name), cargo(&[]).args(run_args(m, p, target, "--example", false)));
        }
        // What a launch builds first (its preLaunchTask).
        for (p, target) in bins.iter().copied().chain(m.targets("example")) {
            t.add(&crate::debug::build_task(target), cargo(&[]).args(crate::debug::build_args(m, p, target)));
        }
    }
    t.add("test", wide(cargo(&["test"]), "--workspace"));
    if metadata.is_some_and(|m| !m.targets("bench").is_empty()) {
        t.add("bench", wide(cargo(&["bench"]), "--workspace"));
    }
    t.add("clippy", wide(cargo(&["clippy", "--all-targets"]), "--workspace"));
    t.add("fmt", wide(cargo(&["fmt"]), "--all"));
    if tools.cargo_audit.is_some() {
        t.add("audit", cargo(&["audit"]));
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{ok, METADATA};
    use crate::tests::line;

    fn table(t: &Tasks) -> Vec<(String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    fn rows(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn a_workspaces_targets_are_its_tasks() {
        let m = Metadata::parse(&ok(METADATA)).unwrap();
        let tools = Tools { cargo: Some("cargo".into()), cargo_audit: Some("/c/cargo-audit".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(Some(&m), true, &tools)),
            rows(&[
                ("build", "cargo build --workspace"),
                ("run orbit", "cargo run -p orbit --bin orbit"),
                ("run probe", "cargo run -p orbit --bin probe"),
                ("example demo", "cargo run -p orbit --example demo"),
                ("build orbit", "cargo build -p orbit --bin orbit"),
                ("build probe", "cargo build -p orbit --bin probe"),
                ("build example demo", "cargo build -p orbit --example demo"),
                ("test", "cargo test --workspace"),
                ("clippy", "cargo clippy --all-targets --workspace"),
                ("fmt", "cargo fmt --all"),
                ("audit", "cargo audit"),
            ])
        );
    }

    #[test]
    fn one_package_with_one_binary_just_runs() {
        let target = |name: &str, kind: &str, features: &[&str]| Target { name: name.into(), kind: vec![kind.into()], src_path: String::new(), required_features: features.iter().map(|f| f.to_string()).collect() };
        let m = Metadata {
            packages: vec![Package { name: "orbit".into(), targets: vec![target("orbit", "bin", &[]), target("gui", "example", &["gui"]), target("speed", "bench", &[])], ..Package::default() }],
            ..Metadata::default()
        };
        let tools = Tools { cargo: Some("/c/cargo".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(Some(&m), false, &tools)),
            rows(&[
                ("build", "/c/cargo build"),
                ("run", "/c/cargo run"),
                ("example gui", "/c/cargo run --example gui --features gui"),
                ("build orbit", "/c/cargo build --bin orbit"),
                ("build example gui", "/c/cargo build --example gui --features gui"),
                ("test", "/c/cargo test"),
                ("bench", "/c/cargo bench"),
                ("clippy", "/c/cargo clippy --all-targets"),
                ("fmt", "/c/cargo fmt"),
            ])
        );
        // Before cargo has answered: what needs no targets.
        assert_eq!(table(&tasks(None, false, &Tools::default())).iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["build", "test", "clippy", "fmt"]);
    }
}
