//! The tasks (`SPC p t`): each of `composer.json`'s scripts (`composer
//! run-script`), the everyday ones first; the project's own tools where
//! no script runs them -- Pest or PHPUnit as `test`, PHPStan, Pint or PHP
//! CS Fixer as `format`; Laravel's `serve`; and Composer's install,
//! update, outdated and audit. The project's `.env` goes to each; a task
//! of the same name in `.fenix/tools.json` wins.

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::project::Project;
use crate::tools::Tools;

/// The scripts listed first, in this order; the rest after, by name.
pub const FIRST: &[&str] = &["test", "lint", "analyse", "analyze", "format", "fix", "serve", "dev", "build"];

/// The scripts Composer runs itself, on its events (`post-autoload-dump`,
/// `pre-install-cmd`): not tasks.
pub fn is_hook(name: &str) -> bool {
    name.starts_with("pre-") || name.starts_with("post-")
}

/// The scripts run on their own, the everyday ones first.
pub fn scripts(project: &Project) -> Vec<&str> {
    let mut names: Vec<&str> = project.scripts.iter().map(String::as_str).filter(|s| !is_hook(s)).collect();
    names.sort_by_key(|s| (FIRST.iter().position(|f| f == s).unwrap_or(FIRST.len()), *s));
    names
}

pub fn tasks(project: &Project, tools: &Tools, root: &str) -> Tasks {
    let mut t = Tasks::default();
    let composer = tools.composer();
    let scripts = scripts(project);
    for s in &scripts {
        t.add(s, composer.clone().args(["run-script", s]));
    }
    let has = |names: &[&str]| names.iter().any(|n| scripts.contains(n));
    let own = |path: &Option<String>, bin: &str| path.as_deref().filter(|_| project.has(bin)).map(Command::new);
    if !has(&["test"]) {
        if let Some(cmd) = own(&tools.pest, "pest").or_else(|| own(&tools.phpunit, "phpunit")) {
            t.add("test", cmd);
        }
    }
    if let (false, Some(cmd)) = (has(&["analyse", "analyze", "phpstan"]), own(&tools.phpstan, "phpstan")) {
        t.add("phpstan", cmd.args(["analyse", "--no-progress"]));
    }
    if !has(&["format", "fix", "pint", "cs-fix"]) {
        match (own(&tools.pint, "pint"), own(&tools.php_cs_fixer, "php-cs-fixer")) {
            (Some(pint), _) => t.add("format", pint),
            (None, Some(fixer)) => t.add("format", fixer.arg("fix")),
            (None, None) => {}
        }
    }
    if project.artisan {
        t.add("serve", tools.php().args(["artisan", "serve"]));
    }
    if project.composer {
        t.add("install", composer.clone().arg("install"));
        t.add("update", composer.clone().arg("update"));
        t.add("outdated", composer.clone().args(["outdated", "--direct"]));
        t.add("audit", composer.arg("audit"));
    }
    t.env_file(&format!("{root}/.env"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, COMPOSER};
    use crate::tests::line;

    fn table(t: &Tasks) -> Vec<(String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    fn rows(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn scripts_first_then_the_tools_no_script_runs() {
        let mut project = Project::parse(COMPOSER).unwrap();
        project.bins = ["phpunit", "phpstan", "pint"].iter().map(|b| b.to_string()).collect();
        project.artisan = true;
        let tools = Tools { php: Some("/usr/bin/php".into()), composer: Some("/c/composer".into()), phpunit: Some("/p/vendor/bin/phpunit".into()), phpstan: Some("/p/vendor/bin/phpstan".into()), pint: Some("/p/vendor/bin/pint".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(&project, &tools, "/nowhere")),
            rows(&[
                ("test", "/c/composer run-script test"),
                ("analyse", "/c/composer run-script analyse"),
                ("format", "/p/vendor/bin/pint"),
                ("serve", "/usr/bin/php artisan serve"),
                ("install", "/c/composer install"),
                ("update", "/c/composer update"),
                ("outdated", "/c/composer outdated --direct"),
                ("audit", "/c/composer audit"),
            ]),
            "the scripts run PHPUnit and PHPStan; post-autoload-dump is Composer's"
        );
    }

    #[test]
    fn without_scripts_the_projects_own_tools_and_its_env() {
        let dir = temp("tasks");
        std::fs::write(dir.join(".env"), "APP_ENV=testing\n").unwrap();
        let project = Project { composer: true, bins: ["pest", "phpunit", "phpstan", "php-cs-fixer"].iter().map(|b| b.to_string()).collect(), ..Project::default() };
        let tools = Tools { pest: Some("/p/vendor/bin/pest".into()), phpunit: Some("/p/vendor/bin/phpunit".into()), phpstan: Some("/p/vendor/bin/phpstan".into()), php_cs_fixer: Some("/p/vendor/bin/php-cs-fixer".into()), pint: Some("/h/pint".into()), ..Tools::default() };
        let t = tasks(&project, &tools, &dir.to_string_lossy());
        let names: Vec<String> = table(&t).into_iter().map(|(n, l)| format!("{n} = {l}")).collect();
        assert_eq!(
            names[..3],
            ["test = /p/vendor/bin/pest", "phpstan = /p/vendor/bin/phpstan analyse --no-progress", "format = /p/vendor/bin/php-cs-fixer fix"],
            "Pest before PHPUnit; a Pint that isn't the project's isn't its formatter"
        );
        assert!(t.resolved().iter().all(|t| t.cmd.env == [("APP_ENV".to_string(), "testing".to_string())]));
        assert!(tasks(&Project::default(), &Tools::default(), "/nowhere").tasks.is_empty(), "no composer.json, no tasks");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
