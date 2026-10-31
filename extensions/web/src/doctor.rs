//! The project doctor's JavaScript checks: Node held to what the project
//! pins (`.nvmrc`, `.node-version`) or asks (`engines.node`), the package
//! manager the lockfile names (at corepack's `packageManager` version),
//! `node_modules` in step with the lockfile, the language servers, the
//! project's ESLint and its formatter, and js-debug. Each that isn't
//! right has a fix row that installs it.

use std::collections::BTreeSet;
use std::path::Path;

use fenix_extension_api::{Check, Health, Section};
use fenix_lang_kit::serde_json::{self, Value};
use fenix_lang_kit::{Fix, Program};

use crate::project::{local_version, Manager, Project};
use crate::tools::{self, hints, Tools};

/// The fix rows' commands, in the manifest.
pub const INSTALL_NODE: &str = "web.install-node";
pub const INSTALL_MANAGER: &str = "web.install-manager";
pub const INSTALL: &str = "web.install";
pub const INSTALL_TSSERVER: &str = "web.install-tsserver";
pub const INSTALL_SERVERS: &str = "web.install-servers";
pub const INSTALL_TAILWIND: &str = "web.install-tailwind";
pub const INSTALL_JS_DEBUG: &str = "web.install.js-debug";

/// `name`, looked for where npm, pnpm, bun and Volta put programs too.
fn program(name: &str, label: &str) -> Program {
    hints().into_iter().fold(Program::new(name, label), Program::hint)
}

fn fix(program: Program, label: &str, command: &str, cmd: Option<fenix_extension_api::Command>) -> Program {
    match cmd {
        Some(cmd) => program.fix(label, command, Fix::Task(cmd)),
        None => program,
    }
}

/// The checks for `project`, with what's on this machine. `eslint_server`:
/// ESLint's server is wanted (`web.eslint` is `server`).
pub fn programs(project: &Project, tools: &Tools, eslint_server: bool) -> Vec<Program> {
    let mut out = Vec::new();

    let node = Program::new("node", "Node").hint(tools::NODE_SETTING);
    let node = hints().into_iter().fold(node, Program::hint);
    let wants = project.node_wants();
    let pinned = project.node_file.is_some();
    let node = match &wants {
        Some((w, _)) => node.wants(w),
        None => node.at_least("18"),
    };
    out.push(fix(node, "Install the Node the project asks for", INSTALL_NODE, tools.install_node(wants.as_ref().map(|(w, _)| w.as_str()), pinned)));

    // The manager: at the version corepack's packageManager pins (its
    // major), which `corepack enable` makes the one on PATH.
    let m = project.manager;
    let mut pm = program(m.program(), "Package manager");
    let pinned = project.package_manager.as_ref().filter(|(n, _)| n == m.program());
    if let Some((_, v)) = pinned {
        pm = pm.wants(&format!("^{v}"));
    }
    let install = match (pinned, &tools.corepack, m) {
        (_, _, Manager::Npm) => None,
        (Some(_), Some(corepack), Manager::Pnpm | Manager::Yarn | Manager::Berry) => Some(fenix_extension_api::Command::new(corepack).args(["enable", m.program()])),
        (_, _, Manager::Bun) => tools.install_global(&["bun"]),
        _ => tools.install_global(&[m.program()]),
    };
    let label = if install.as_ref().is_some_and(|c| c.args.first().is_some_and(|a| a == "enable")) { "Enable corepack".to_string() } else { format!("Install {}", m.program()) };
    if project.package_json {
        out.push(fix(pm, &label, INSTALL_MANAGER, install));
    }

    let ts = program(tools::TSSERVER, "TypeScript server").version_args(Vec::<String>::new()).optional();
    out.push(fix(ts, "Install typescript-language-server", INSTALL_TSSERVER, tools.install_global(&["typescript-language-server", "typescript"])));
    let extracted = program(tools::CSS_SERVER, "CSS and HTML servers").version_args(Vec::<String>::new()).optional();
    out.push(fix(extracted, "Install vscode-langservers-extracted", INSTALL_SERVERS, tools.install_global(&["vscode-langservers-extracted"])));
    if eslint_server && project.eslint {
        let s = program(tools::ESLINT_SERVER, "ESLint server").version_args(Vec::<String>::new()).optional();
        out.push(fix(s, "Install vscode-langservers-extracted", INSTALL_SERVERS, tools.install_global(&["vscode-langservers-extracted"])));
    }
    if project.tailwind {
        let s = program(tools::TAILWIND_SERVER, "Tailwind CSS server").version_args(Vec::<String>::new()).optional();
        out.push(fix(s, "Install @tailwindcss/language-server", INSTALL_TAILWIND, tools.install_global(&["@tailwindcss/language-server"])));
    }
    out
}

/// What differs between two lists of `(where, version)`: the names, a
/// few, and how many more.
fn differs(what: &str, mut names: Vec<String>) -> String {
    names.sort();
    names.dedup();
    let shown = names.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
    let more = if names.len() > 3 { format!(" and {} more", names.len() - 3) } else { String::new() };
    format!("node_modules differs from {what}: {shown}{more}")
}

/// npm's lockfile and the copy it keeps of what it installed
/// (`node_modules/.package-lock.json`): each package at the same
/// version. One that's optional, and so may not be for this machine, may
/// be missing.
pub fn npm_in_step(lock: &str, installed: &str) -> Result<(), String> {
    let read = |t: &str| serde_json::from_str::<Value>(t).ok().and_then(|v| v["packages"].as_object().cloned()).unwrap_or_default();
    let (lock, installed) = (read(lock), read(installed));
    let mut off = Vec::new();
    for (path, p) in lock.iter().filter(|(k, _)| !k.is_empty()) {
        let name = path.rsplit("node_modules/").next().unwrap_or(path).to_string();
        match installed.get(path) {
            None if p["optional"] == true || p["devOptional"] == true || p["peer"] == true => {}
            None => off.push(name),
            Some(i) if i["version"] != p["version"] => off.push(name),
            Some(_) => {}
        }
    }
    for path in installed.keys().filter(|k| !lock.contains_key(*k)) {
        off.push(path.rsplit("node_modules/").next().unwrap_or(path).to_string());
    }
    if off.is_empty() {
        Ok(())
    } else {
        Err(differs("package-lock.json", off))
    }
}

/// A Yarn 1 lockfile's entries: each pattern its headers name
/// (`chalk@^4.1.0, chalk@^4.0.0:`).
pub fn yarn_patterns(lock: &str) -> BTreeSet<String> {
    lock.lines()
        .filter(|l| !l.starts_with([' ', '#']) && l.ends_with(':'))
        .flat_map(|l| l.trim_end_matches(':').split(", ").map(|p| p.trim_matches('"').to_string()).collect::<Vec<_>>())
        .collect()
}

/// `yarn.lock` and what Yarn 1 says it installed from (`node_modules/
/// .yarn-integrity`'s `lockfileEntries`).
pub fn yarn_in_step(lock: &str, integrity: &str) -> Result<(), String> {
    let entries: BTreeSet<String> = serde_json::from_str::<Value>(integrity).ok().and_then(|v| v["lockfileEntries"].as_object().map(|m| m.keys().cloned().collect())).unwrap_or_default();
    let patterns = yarn_patterns(lock);
    let off: Vec<String> = patterns.symmetric_difference(&entries).map(|p| p.rsplit_once('@').map(|(n, _)| n).unwrap_or(p).to_string()).collect();
    if off.is_empty() {
        Ok(())
    } else {
        Err(differs("yarn.lock", off))
    }
}

/// Whether `a` was changed after `b`.
fn newer(a: &Path, b: &Path) -> bool {
    let at = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    matches!((at(a), at(b)), (Some(a), Some(b)) if a > b)
}

/// `node_modules` held to the lockfile, each manager by what it leaves
/// there: npm's and pnpm's copies of the lockfile, Yarn 1's integrity
/// file, and for berry and bun, the lockfile not changed since the last
/// install. Its fix runs the install.
pub fn node_modules(project: &Project, root: &str) -> Option<Check> {
    if !project.package_json || project.deps.is_empty() {
        return None;
    }
    let dir = Path::new(root);
    let label = "node_modules";
    let check = |health: Health, detail: String| {
        let c = Check::new(Section::Project, label, health, detail);
        if health == Health::Ok {
            c
        } else {
            c.fix(&format!("Install packages ({} install)", project.manager.program()), INSTALL)
        }
    };
    if !dir.join("node_modules").is_dir() && !dir.join(".pnp.cjs").is_file() {
        return Some(check(Health::Warn, "there's no node_modules yet".into()));
    }
    let Some(lockfile) = project.lockfile.as_deref() else {
        return Some(check(Health::Warn, "there's no lockfile: installs may differ from one machine to the next".into()));
    };
    let read = |p: &str| std::fs::read_to_string(dir.join(p)).unwrap_or_default();
    let said = match project.manager {
        Manager::Npm => npm_in_step(&read(lockfile), &read("node_modules/.package-lock.json")),
        Manager::Pnpm if read(lockfile) == read("node_modules/.pnpm/lock.yaml") => Ok(()),
        Manager::Pnpm => Err("node_modules differs from pnpm-lock.yaml".into()),
        Manager::Yarn => yarn_in_step(&read(lockfile), &read("node_modules/.yarn-integrity")),
        Manager::Berry if newer(&dir.join(lockfile), &dir.join(".yarn/install-state.gz")) => Err("yarn.lock changed since the last yarn install".into()),
        Manager::Bun if newer(&dir.join(lockfile), &dir.join("node_modules")) => Err(format!("{lockfile} changed since the last bun install")),
        _ => Ok(()),
    };
    Some(match said {
        Ok(()) => check(Health::Ok, format!("in step with {lockfile}")),
        Err(why) => check(Health::Warn, why),
    })
}

/// The project's own tool `package`, installed in `node_modules` when it
/// asks for it: what lints and formats is the project's version.
pub fn local(project: &Project, root: &str, label: &str, package: &str) -> Check {
    match local_version(root, package) {
        Some(v) => Check::new(Section::Project, label, Health::Ok, format!("{v} (node_modules/{package})")),
        None => Check::new(Section::Project, label, Health::Warn, format!("the project uses {label}, but {package} isn't in node_modules")).fix(&format!("Install packages ({} install)", project.manager.program()), INSTALL),
    }
}

/// js-debug in the tools folder, the debugger: installed or not.
pub fn js_debug(installed: Option<&str>) -> Check {
    match installed {
        Some(version) => Check::new(Section::Toolchain, "Debugger", Health::Ok, format!("js-debug {version} (in Fenix's tools folder)")),
        None => Check::new(Section::Toolchain, "Debugger", Health::Warn, "js-debug isn't installed").fix("Install js-debug", INSTALL_JS_DEBUG),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, PACKAGE};
    use fenix_lang_kit::Output;

    fn rows(programs: &[Program]) -> Vec<(String, String, bool, Option<String>)> {
        programs.iter().map(|p| (p.label.clone(), p.name.clone(), p.required, p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    fn out(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    #[test]
    fn node_is_held_to_engines_and_pnpm_to_packagemanager() {
        let project = Project::parse(PACKAGE, &["pnpm-lock.yaml"]).unwrap();
        let tools = Tools { npm: Some("/n/npm".into()), corepack: Some("/n/corepack".into()), fnm: Some("/f/fnm".into()), ..Tools::default() };
        let p = programs(&project, &tools, false);
        assert_eq!(
            rows(&p),
            [
                ("Node".into(), "node".into(), true, Some(INSTALL_NODE.into())),
                ("Package manager".into(), "pnpm".into(), true, Some(INSTALL_MANAGER.into())),
                ("TypeScript server".into(), tools::TSSERVER.into(), false, Some(INSTALL_TSSERVER.into())),
                ("CSS and HTML servers".into(), tools::CSS_SERVER.into(), false, Some(INSTALL_SERVERS.into())),
                ("Tailwind CSS server".into(), tools::TAILWIND_SERVER.into(), false, Some(INSTALL_TAILWIND.into())),
            ],
            "ESLint lints on save: its server isn't wanted"
        );
        assert_eq!(p[0].hints[0], "${setting:web.node}", "the Node you chose first");
        let c = p[0].evaluate(Some("/n/node"), Some(&out("v18.20.4\n")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Bad, "18.20.4, but the project needs >=20 (/n/node)"));
        assert_eq!(p[0].evaluate(Some("/n/node"), Some(&out("v22.22.0"))).health, Health::Ok);
        match &p[0].fix {
            Some((_, _, Fix::Task(cmd))) => assert_eq!(crate::tests::line(cmd), "/f/fnm install 20"),
            other => panic!("{other:?}"),
        }
        // pnpm at 9, as corepack pins it.
        assert_eq!(p[1].evaluate(Some("/n/pnpm"), Some(&out("10.28.0"))).health, Health::Bad);
        let c = p[1].evaluate(Some("/n/pnpm"), Some(&out("9.15.1")));
        assert_eq!(c.health, Health::Ok);
        match &p[1].fix {
            Some((label, _, Fix::Task(cmd))) => assert_eq!((label.as_str(), crate::tests::line(cmd).as_str()), ("Enable corepack", "/n/corepack enable pnpm")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn without_corepack_a_manager_is_installed_by_npm_and_npm_by_nobody() {
        let project = Project::parse(r#"{"devDependencies": {"eslint": "^9"}}"#, &["yarn.lock"]).unwrap();
        let tools = Tools { npm: Some("/n/npm".into()), ..Tools::default() };
        let p = programs(&project, &tools, true);
        assert_eq!(p[1].fix.as_ref().map(|f| f.0.as_str()), Some("Install yarn"));
        assert!(p.iter().any(|p| p.label == "ESLint server"));
        let npm = Project::parse("{}", &[]).unwrap();
        let p = programs(&npm, &Tools::default(), false);
        assert!(p.iter().all(|p| p.fix.is_none()), "nothing to install with: {:?}", rows(&p));
        assert_eq!(p[0].wants.as_ref().map(|w| w.to_string()).as_deref(), Some(">=18"));
    }

    #[test]
    fn npms_hidden_lockfile_and_yarns_integrity_say_whether_node_modules_is_in_step() {
        let lock = include_str!("samples/npm-package-lock.json");
        let hidden = include_str!("samples/npm-hidden-lock.json");
        assert_eq!(npm_in_step(lock, hidden), Ok(()));
        let older = hidden.replace("\"version\": \"4.17.20\"", "\"version\": \"4.17.19\"");
        assert_eq!(npm_in_step(lock, &older), Err("node_modules differs from package-lock.json: lodash".into()));
        assert!(npm_in_step(lock, "{}").unwrap_err().ends_with("and 7 more"), "nothing installed");
        let ylock = include_str!("samples/yarn1.lock");
        let integrity = include_str!("samples/yarn1-integrity.json");
        assert_eq!(yarn_patterns(ylock).len(), 10);
        assert_eq!(yarn_in_step(ylock, integrity), Ok(()));
        let more = format!("{ylock}\nzod@^3.0.0:\n  version \"3.25.0\"\n");
        assert_eq!(yarn_in_step(&more, integrity), Err("node_modules differs from yarn.lock: zod".into()));
    }

    #[test]
    fn node_modules_missing_stale_or_in_step() {
        let dir = temp("doctor");
        let root = dir.to_string_lossy().into_owned();
        std::fs::write(dir.join("package.json"), r#"{"dependencies": {"lodash": "4.17.20"}}"#).unwrap();
        std::fs::write(dir.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
        let p = Project::read(&root);
        let c = node_modules(&p, &root).unwrap();
        assert_eq!((c.health, c.detail.as_str()), (Health::Warn, "there's no node_modules yet"));
        assert_eq!(c.fix, Some(("Install packages (pnpm install)".into(), INSTALL.into())));
        std::fs::create_dir_all(dir.join("node_modules/.pnpm")).unwrap();
        std::fs::write(dir.join("node_modules/.pnpm/lock.yaml"), "lockfileVersion: '6.0'\n").unwrap();
        assert_eq!(node_modules(&p, &root).unwrap().detail, "node_modules differs from pnpm-lock.yaml");
        std::fs::write(dir.join("node_modules/.pnpm/lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
        let c = node_modules(&p, &root).unwrap();
        assert_eq!((c.health, c.fix), (Health::Ok, None));
        assert!(node_modules(&Project::parse("{}", &[]).unwrap(), &root).is_none(), "no dependencies, nothing to install");
        let c = local(&p, &root, "Prettier", "prettier");
        assert_eq!((c.health, c.fix.map(|f| f.1)), (Health::Warn, Some(INSTALL.into())));
        assert_eq!(js_debug(None).fix, Some(("Install js-debug".into(), INSTALL_JS_DEBUG.into())));
        assert_eq!(js_debug(Some("1.140.0")).health, Health::Ok);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
