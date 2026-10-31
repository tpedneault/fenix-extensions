//! Launches worked out from the project (`launches-set`, 0.8.6), offered
//! by `SPC u u` beside the launch files: each of `[project.scripts]` by
//! what it runs (`orbit = "orbit.cli:main"`, as debugpy's `code`), and a
//! package with a `__main__.py` by `python -m`. A launch file can't know
//! these.

use fenix_extension_api::serde_json::json;
use fenix_extension_api::LaunchConfig;

use crate::project::Project;

/// Fenix's name for what debugpy debugs.
const PYTHON: &[&str] = &["python"];

/// What a script's entry point runs, as code for `python -c`: `pkg.cli:main`
/// is `main()` imported from `pkg.cli`, its value the exit code; an
/// object's attribute (`pkg.cli:app.run`) is reached from its object.
pub fn entry_code(target: &str) -> Option<String> {
    let (module, attr) = target.split_once(':')?;
    let (module, attr) = (module.trim(), attr.trim());
    let ok = |s: &str| !s.is_empty() && s.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || c == '_'));
    // An extra (`pkg:main [cli]`) is the installer's, not the code's.
    let attr = attr.split_whitespace().next().unwrap_or(attr);
    if !ok(module) || !ok(attr) {
        return None;
    }
    let head = attr.split('.').next().unwrap_or(attr);
    Some(format!("import sys; from {module} import {head}; sys.exit({attr}())"))
}

/// The project's launches: its scripts, then its package's `__main__`.
pub fn launches(project: &Project) -> Vec<LaunchConfig> {
    let mut out = Vec::new();
    for (name, target) in &project.script_targets {
        let Some(code) = entry_code(target) else { continue };
        out.push(LaunchConfig::launch(&format!("Python: {name} (script)"), PYTHON).cwd("${project}").env("PYTHONUNBUFFERED", "1").attributes(&json!({ "code": code, "justMyCode": true })));
    }
    if let Some(module) = &project.main_module {
        out.push(LaunchConfig::launch(&format!("Python: python -m {module}"), PYTHON).cwd("${project}").env("PYTHONUNBUFFERED", "1").attributes(&json!({ "module": module, "justMyCode": true })));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_extension_api::serde_json::Value;

    #[test]
    fn an_entry_point_is_called_as_its_script_would_be() {
        assert_eq!(entry_code("orbit.cli:main").as_deref(), Some("import sys; from orbit.cli import main; sys.exit(main())"));
        assert_eq!(entry_code("orbit:app.run [cli]").as_deref(), Some("import sys; from orbit import app; sys.exit(app.run())"));
        assert_eq!(entry_code("orbit"), None, "no object");
        assert_eq!(entry_code("orbit:main; rm"), None, "only names");
    }

    #[test]
    fn each_script_and_the_main_package_is_a_launch() {
        let project = Project { script_targets: vec![("orbit".into(), "orbit.cli:main".into()), ("bad".into(), "nope".into())], main_module: Some("orbit".into()), ..Project::default() };
        let l = launches(&project);
        assert_eq!(l.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Python: orbit (script)", "Python: python -m orbit"]);
        let attrs: Value = fenix_extension_api::serde_json::from_str(&l[0].attributes).unwrap();
        assert_eq!(attrs["code"], "import sys; from orbit.cli import main; sys.exit(main())");
        assert_eq!((l[0].program.as_deref(), l[0].cwd.as_deref(), l[0].languages.clone()), (None, Some("${project}"), vec!["python".to_string()]), "debugpy's code runs, not a program");
        let attrs: Value = fenix_extension_api::serde_json::from_str(&l[1].attributes).unwrap();
        assert_eq!(attrs["module"], "orbit");
        assert!(launches(&Project::default()).is_empty());
    }
}
