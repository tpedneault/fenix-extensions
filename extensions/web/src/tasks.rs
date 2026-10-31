//! The tasks (`SPC p t`): each of `package.json`'s scripts, run with the
//! project's package manager (`pnpm run dev`), the everyday ones first,
//! and `install`. A task of the same name in `.fenix/tools.json` wins.
//! Saving `package.json` makes them again.

use fenix_lang_kit::Tasks;

use crate::project::Project;
use crate::tools::Tools;

/// The scripts listed first, in this order; the rest after, by name.
pub const FIRST: &[&str] = &["dev", "start", "build", "test", "lint", "format", "typecheck", "preview"];

/// What npm runs scripts around itself.
const LIFECYCLE: &[&str] = &["install", "uninstall", "publish", "pack", "version"];

/// A script's place in the list.
fn rank(name: &str) -> (usize, &str) {
    (FIRST.iter().position(|f| *f == name).unwrap_or(FIRST.len()), name)
}

/// The scripts run on their own, the everyday ones first: `prebuild` and
/// `postinstall` run with what they're named for.
pub fn scripts(project: &Project) -> Vec<&str> {
    let names: Vec<&str> = project.scripts.iter().map(|(k, _)| k.as_str()).collect();
    let hook = |k: &str| [k.strip_prefix("pre"), k.strip_prefix("post")].into_iter().flatten().any(|of| names.contains(&of) || LIFECYCLE.contains(&of));
    let mut scripts: Vec<&str> = names.iter().copied().filter(|k| !hook(k)).collect();
    scripts.sort_by_key(|s| rank(s));
    scripts
}

pub fn tasks(project: &Project, tools: &Tools) -> Tasks {
    let mut t = Tasks::default();
    if !project.package_json {
        return t;
    }
    let pm = tools.manager(project.manager);
    for s in scripts(project) {
        t.add(s, pm.clone().args(["run", s]));
    }
    t.add("install", pm.arg("install"));
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::PACKAGE;
    use crate::tests::line;

    #[test]
    fn scripts_run_with_the_projects_manager_the_everyday_ones_first() {
        let project = Project::parse(PACKAGE, &["pnpm-lock.yaml"]).unwrap();
        let tools = Tools { pnpm: Some("/n/pnpm".into()), ..Tools::default() };
        let t = tasks(&project, &tools);
        let rows: Vec<(String, String)> = t.tasks.iter().map(|t| (t.name.clone(), line(&t.cmd))).collect();
        assert_eq!(
            rows,
            [
                ("dev".into(), "/n/pnpm run dev".into()),
                ("build".into(), "/n/pnpm run build".into()),
                ("test".into(), "/n/pnpm run test".into()),
                ("lint".into(), "/n/pnpm run lint".into()),
                ("install".into(), "/n/pnpm install".into()),
            ]
        );
        let hooks = Project::parse(r#"{"scripts": {"prebuild": "rm -rf dist", "build": "tsc", "postinstall": "patch-package", "e2e": "playwright test", "prettier": "prettier -w .", "preview": "vite preview"}}"#, &["bun.lock"]).unwrap();
        let names: Vec<String> = tasks(&hooks, &Tools::default()).tasks.iter().map(|t| format!("{} = {}", t.name, line(&t.cmd))).collect();
        assert_eq!(names, ["build = bun run build", "preview = bun run preview", "e2e = bun run e2e", "prettier = bun run prettier", "install = bun install"], "npm runs pre and post scripts itself");
        assert!(tasks(&Project::default(), &tools).tasks.is_empty(), "no package.json, no tasks");
    }
}
