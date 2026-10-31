//! The project doctor's PHP checks: PHP held to `composer.json`'s
//! `require.php`, Composer, `vendor` in step with `composer.lock`, the
//! language server, and the project's Pint, PHP CS Fixer and PHPStan
//! where it uses them. Each that isn't right has a fix row: npm installs
//! Intelephense, the pack Phpactor's release, and `composer install` the
//! project's packages and tools.

use std::path::Path;

use fenix_extension_api::{Check, Command, Health, Section};
use fenix_lang_kit::{Fix, Program};

use crate::project::{constraint, vendor_in_step, Project};
use crate::tools::{hints, Server, Tools, INTELEPHENSE, PHPACTOR};

/// The fix rows' commands, in the manifest.
pub const INSTALL: &str = "php.install";
pub const INSTALL_INTELEPHENSE: &str = "php.install-intelephense";
pub const INSTALL_PHPACTOR: &str = "php.install.phpactor";

/// The oldest PHP still given security fixes, wanted when the project
/// doesn't say.
pub const LEAST_PHP: &str = "8.2";

/// `name`, looked for in `vendor/bin` and where Composer, npm and Herd
/// put programs.
fn program(name: &str, label: &str) -> Program {
    hints(name).into_iter().fold(Program::new(name, label), Program::hint)
}

/// `composer install`, the fix for what the project's packages bring.
pub fn composer_install(tools: &Tools) -> Command {
    tools.composer().arg("install")
}

/// The checks for `project`, with what's on this machine; `phar`: the
/// pack's Phpactor is installed, and checked on its own ([`phpactor`]).
pub fn programs(project: &Project, tools: &Tools, server: Server, phar: bool) -> Vec<Program> {
    let mut out = Vec::new();

    let php = program("php", "PHP");
    out.push(match project.php.as_deref().map(constraint) {
        Some(wants) => php.wants(&wants),
        None => php.at_least(LEAST_PHP),
    });
    let composer = program("composer", "Composer").env("COMPOSER_NO_INTERACTION", "1");
    out.push(if project.composer { composer } else { composer.optional() });

    // The server: found only, as asking its version would start it.
    match server {
        Server::Intelephense => {
            let s = program(INTELEPHENSE, "Intelephense").version_args(Vec::<String>::new()).optional();
            out.push(match tools.install_intelephense() {
                Some(cmd) => s.fix("Install Intelephense", INSTALL_INTELEPHENSE, Fix::Task(cmd)),
                None => s,
            });
        }
        Server::Phpactor if !phar => out.push(program("phpactor", "Phpactor").optional().fix("Install Phpactor", INSTALL_PHPACTOR, Fix::Install("phpactor".into()))),
        Server::Phpactor | Server::Off => {}
    }

    // The project's own tools: there once `composer install` has run.
    let install = |p: Program| if project.composer { p.fix("Install packages (composer install)", INSTALL, Fix::Task(composer_install(tools))) } else { p };
    for (uses, name, label) in [(project.uses_pint(), "pint", "Pint"), (project.uses_cs_fixer() && !project.uses_pint(), "php-cs-fixer", "PHP CS Fixer"), (project.uses_phpstan(), "phpstan", "PHPStan")] {
        if uses {
            out.push(install(program(name, label).section(Section::Project).optional()));
        }
    }
    out
}

/// Phpactor's PHAR in the tools folder, when it's the server wanted:
/// installed, or not.
pub fn phpactor(installed: bool) -> Check {
    if installed {
        Check::new(Section::Toolchain, "Phpactor", Health::Ok, format!("{PHPACTOR} (in Fenix's tools folder)"))
    } else {
        Check::new(Section::Toolchain, "Phpactor", Health::Warn, "Phpactor isn't installed").fix("Install Phpactor", INSTALL_PHPACTOR)
    }
}

/// `vendor` held to `composer.lock`, by what Composer says it installed
/// (`vendor/composer/installed.json`). Its fix runs `composer install`.
pub fn vendor(project: &Project, root: &str) -> Option<Check> {
    if !project.composer || project.packages.is_empty() {
        return None;
    }
    let dir = Path::new(root);
    let check = |health: Health, detail: String| {
        let c = Check::new(Section::Project, "vendor", health, detail);
        if health == Health::Ok {
            c
        } else {
            c.fix("Install packages (composer install)", INSTALL)
        }
    };
    if !dir.join("vendor").is_dir() {
        return Some(check(Health::Warn, "there's no vendor folder yet".into()));
    }
    if !project.lock {
        return Some(check(Health::Warn, "there's no composer.lock: installs may differ from one machine to the next".into()));
    }
    let read = |p: &str| std::fs::read_to_string(dir.join(p)).unwrap_or_default();
    Some(match vendor_in_step(&read("composer.lock"), &read("vendor/composer/installed.json")) {
        Ok(()) => check(Health::Ok, "in step with composer.lock".into()),
        Err(why) => check(Health::Warn, why),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, write, COMPOSER};
    use fenix_lang_kit::Output;

    fn rows(programs: &[Program]) -> Vec<(String, String, bool, Option<String>)> {
        programs.iter().map(|p| (p.label.clone(), p.name.clone(), p.required, p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    fn out(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    #[test]
    fn php_is_held_to_composer_json_and_the_tools_it_uses_are_checked() {
        let project = Project::parse(COMPOSER).unwrap();
        let tools = Tools { npm: Some("/n/npm".into()), composer: Some("/c/composer".into()), ..Tools::default() };
        let p = programs(&project, &tools, Server::Intelephense, false);
        assert_eq!(
            rows(&p),
            [
                ("PHP".into(), "php".into(), true, None),
                ("Composer".into(), "composer".into(), true, None),
                ("Intelephense".into(), INTELEPHENSE.into(), false, Some(INSTALL_INTELEPHENSE.into())),
                ("Pint".into(), "pint".into(), false, Some(INSTALL.into())),
                ("PHPStan".into(), "phpstan".into(), false, Some(INSTALL.into())),
            ]
        );
        let c = p[0].evaluate(Some("/usr/bin/php"), Some(&out("PHP 8.1.31 (cli) (built: Nov 21 2024 13:10:17) (NTS)\nCopyright (c) The PHP Group\nZend Engine v4.1.31, Copyright (c) Zend Technologies\n")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Bad, "8.1.31, but the project needs ^8.2 (/usr/bin/php)"));
        assert_eq!(p[0].evaluate(Some("/usr/bin/php"), Some(&out("PHP 8.3.6 (cli) (built: Oct 19 2026 15:16:58) (NTS)"))).health, Health::Ok);
        let c = p[1].evaluate(Some("/c/composer"), Some(&out("Composer version 2.8.12 2025-09-19 13:41:59\nPHP version 8.3.6 (/usr/bin/php)\n")));
        assert_eq!(c.detail, "2.8.12 (/c/composer)");
        assert_eq!(p[3].hints[0], "./vendor/bin", "the project's own first");
        match &p[2].fix {
            Some((_, _, Fix::Task(cmd))) => assert_eq!(crate::tests::line(cmd), "/n/npm install -g intelephense"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn phpactor_is_the_packs_release_and_no_project_asks_less() {
        let p = programs(&Project::default(), &Tools::default(), Server::Phpactor, false);
        assert_eq!(rows(&p), [("PHP".into(), "php".into(), true, None), ("Composer".into(), "composer".into(), false, None), ("Phpactor".into(), "phpactor".into(), false, Some(INSTALL_PHPACTOR.into()))]);
        assert_eq!(p[0].wants.as_ref().map(|w| w.to_string()).as_deref(), Some(">=8.2"));
        assert_eq!(p[2].fix.as_ref().map(|f| f.2.clone()), Some(Fix::Install("phpactor".into())));
        assert_eq!(programs(&Project::default(), &Tools::default(), Server::Phpactor, true).len(), 2, "installed: its own check");
        assert_eq!(programs(&Project::default(), &Tools::default(), Server::Off, false).len(), 2);
        assert_eq!(phpactor(true).detail, "2026.10.07.0 (in Fenix's tools folder)");
        assert_eq!(phpactor(false).fix, Some(("Install Phpactor".into(), INSTALL_PHPACTOR.into())));
        let fixer = Project { cs_fixer_config: Some(".php-cs-fixer.php".into()), ..Project::default() };
        assert_eq!(programs(&fixer, &Tools::default(), Server::Off, false)[2].fix, None, "no composer.json to install it from");
    }

    #[test]
    fn vendor_missing_stale_or_in_step() {
        let dir = temp("doctor");
        let root = dir.to_string_lossy().into_owned();
        write(&dir, &[("composer.json", COMPOSER), ("composer.lock", include_str!("samples/composer.lock"))]);
        let p = Project::read(&root);
        let c = vendor(&p, &root).unwrap();
        assert_eq!((c.health, c.detail.as_str()), (Health::Warn, "there's no vendor folder yet"));
        assert_eq!(c.fix, Some(("Install packages (composer install)".into(), INSTALL.into())));
        write(&dir, &[("vendor/composer/installed.json", r#"{"packages": [{"name": "brick/math", "version": "0.12.3"}]}"#)]);
        assert!(vendor(&p, &root).unwrap().detail.starts_with("vendor differs from composer.lock: laravel/framework"));
        write(&dir, &[("vendor/composer/installed.json", include_str!("samples/installed.json"))]);
        let c = vendor(&p, &root).unwrap();
        assert_eq!((c.health, c.fix), (Health::Ok, None));
        std::fs::remove_file(dir.join("composer.lock")).unwrap();
        assert!(vendor(&Project::read(&root), &root).unwrap().detail.starts_with("there's no composer.lock"));
        assert!(vendor(&Project::default(), &root).is_none(), "no composer.json, nothing to install");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
