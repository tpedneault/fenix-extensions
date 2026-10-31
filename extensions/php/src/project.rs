//! What the project is, as its files say: `composer.json` (the PHP it
//! asks for, its packages, its scripts), whether there's a
//! `composer.lock`, the tools Composer put in `vendor/bin`, their
//! configs, and whether it's a Laravel app (`artisan`).

use std::collections::BTreeSet;
use std::path::Path;

use fenix_lang_kit::serde_json::{self, Value};

/// The project's own tools, as Composer installs them in `vendor/bin`.
pub const BINS: &[&str] = &["phpunit", "pest", "phpstan", "pint", "php-cs-fixer"];

/// PHP CS Fixer's config, by the names it looks for.
pub const CS_FIXER_CONFIGS: &[&str] = &[".php-cs-fixer.php", ".php-cs-fixer.dist.php"];

/// PHPStan's.
pub const PHPSTAN_CONFIGS: &[&str] = &["phpstan.neon", "phpstan.neon.dist", "phpstan.dist.neon"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// A `composer.json` at the top.
    pub composer: bool,
    /// Its `require.php`, as Composer writes it: `^8.2`, `>=8.1`.
    pub php: Option<String>,
    /// The packages it requires, `require` and `require-dev`.
    pub packages: BTreeSet<String>,
    /// Its scripts, by name.
    pub scripts: Vec<String>,
    pub lock: bool,
    /// Which of [`BINS`] are in `vendor/bin`.
    pub bins: BTreeSet<String>,
    /// `pint.json`.
    pub pint_config: bool,
    /// PHP CS Fixer's config, by its name.
    pub cs_fixer_config: Option<String>,
    pub phpstan_config: bool,
    /// `artisan` at the top: Laravel.
    pub artisan: bool,
}

/// Composer's version constraint as lang-kit's [`fenix_lang_kit::Requirement`]
/// reads one: `|` is `||`, `~8.2` is `>=8.2 <9` (Python's `~=`, not npm's
/// `~`), and a stability flag (`@dev`) is left out.
pub fn constraint(text: &str) -> String {
    let alternatives = text.split('|').map(str::trim).filter(|a| !a.is_empty());
    alternatives
        .map(|alt| {
            alt.split([' ', ','])
                .filter(|w| !w.is_empty())
                .map(|w| w.split('@').next().unwrap_or(w))
                .map(|w| match w.strip_prefix('~') {
                    Some(v) if v.split('.').count() == 2 => format!("~={v}"),
                    _ => w.to_string(),
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(" || ")
}

impl Project {
    /// From `composer.json`'s text; the files are the rest's.
    pub fn parse(composer: &str) -> Result<Project, String> {
        let v: Value = serde_json::from_str(composer).map_err(|e| format!("composer.json: {e}"))?;
        let names = |key: &str| v[key].as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
        Ok(Project {
            composer: true,
            php: v["require"]["php"].as_str().map(str::to_string),
            packages: names("require").into_iter().chain(names("require-dev")).filter(|n| n.contains('/')).collect(),
            scripts: names("scripts"),
            ..Project::default()
        })
    }

    /// The project in `root`.
    pub fn read(root: &str) -> Project {
        let dir = Path::new(root);
        let mut p = std::fs::read_to_string(dir.join("composer.json")).ok().and_then(|t| Project::parse(&t).ok()).unwrap_or_default();
        p.composer = dir.join("composer.json").is_file();
        p.lock = dir.join("composer.lock").is_file();
        let bin = dir.join("vendor/bin");
        p.bins = BINS.iter().filter(|b| bin.join(b).is_file() || bin.join(format!("{b}.bat")).is_file()).map(|b| b.to_string()).collect();
        p.pint_config = dir.join("pint.json").is_file();
        p.cs_fixer_config = CS_FIXER_CONFIGS.iter().find(|c| dir.join(c).is_file()).map(|c| c.to_string());
        p.phpstan_config = PHPSTAN_CONFIGS.iter().any(|c| dir.join(c).is_file());
        p.artisan = dir.join("artisan").is_file();
        p
    }

    /// Whether `bin` is the project's own, in `vendor/bin`.
    pub fn has(&self, bin: &str) -> bool {
        self.bins.contains(bin)
    }

    /// The project uses Pint: its config, the package, or its program.
    pub fn uses_pint(&self) -> bool {
        self.pint_config || self.packages.contains("laravel/pint") || self.has("pint")
    }

    pub fn uses_cs_fixer(&self) -> bool {
        self.cs_fixer_config.is_some() || self.packages.contains("friendsofphp/php-cs-fixer") || self.has("php-cs-fixer")
    }

    pub fn uses_phpstan(&self) -> bool {
        self.phpstan_config || self.packages.iter().any(|p| p == "phpstan/phpstan" || p == "larastan/larastan") || self.has("phpstan")
    }
}

/// Each package's version, by name: `composer.lock`'s `packages` and
/// `packages-dev`, or what Composer says it installed
/// (`vendor/composer/installed.json`: an array in Composer 1, `packages`
/// in 2).
fn versions(text: &str) -> Vec<(String, String)> {
    let v: Value = serde_json::from_str(text).unwrap_or_default();
    let lists = [&v["packages"], &v["packages-dev"], &v];
    lists
        .into_iter()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|p| Some((p["name"].as_str()?.to_string(), p["version"].as_str()?.to_string())))
        .collect()
}

/// `vendor` held to `composer.lock`: each locked package installed at its
/// version. What differs, a few by name and how many more.
pub fn vendor_in_step(lock: &str, installed: &str) -> Result<(), String> {
    let installed: BTreeSet<(String, String)> = versions(installed).into_iter().collect();
    let off: Vec<String> = versions(lock).into_iter().filter(|p| !installed.contains(p)).map(|(name, _)| name).collect();
    if off.is_empty() {
        return Ok(());
    }
    let shown = off.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
    let more = if off.len() > 3 { format!(" and {} more", off.len() - 3) } else { String::new() };
    Err(format!("vendor differs from composer.lock: {shown}{more}"))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use fenix_lang_kit::{Requirement, Version};

    /// A fresh folder for a test.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-php-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub fn write(dir: &Path, files: &[(&str, &str)]) {
        for (f, text) in files {
            let path = dir.join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }

    pub const COMPOSER: &str = include_str!("samples/composer.json");

    #[test]
    fn composer_json_says_the_php_packages_and_scripts() {
        let p = Project::parse(COMPOSER).unwrap();
        assert_eq!(p.php.as_deref(), Some("^8.2"));
        assert!(p.packages.contains("laravel/framework") && p.packages.contains("phpunit/phpunit") && !p.packages.contains("ext-mbstring"), "{:?}", p.packages);
        assert_eq!(p.scripts, ["analyse", "post-autoload-dump", "test"]);
        assert!(p.uses_pint() && p.uses_phpstan() && !p.uses_cs_fixer());
        assert!(Project::parse("{").unwrap_err().starts_with("composer.json:"));
    }

    #[test]
    fn composers_constraints_read_as_requirements() {
        let ok = |c: &str, v: &str| Requirement::parse(&constraint(c)).unwrap().matches(&Version::parse(v).unwrap());
        assert_eq!(constraint("^8.1 | ^8.2"), "^8.1 || ^8.2");
        assert!(ok("~8.2", "8.4.1") && !ok("~8.2", "9.0.0"), "Composer's ~8.2 is >=8.2 <9");
        assert!(ok("~8.2.3", "8.2.9") && !ok("~8.2.3", "8.3.0"));
        assert!(ok(">=8.1 <8.4", "8.3.6") && !ok(">=8.1,<8.4", "8.4.0"));
        assert!(ok("8.3.*", "8.3.6") && !ok("8.3.*", "8.4.0"));
        assert!(ok("^7.4 || ^8.0", "8.3.6") && ok("^8.2@dev", "8.2.0"));
    }

    #[test]
    fn the_projects_files_say_its_tools() {
        let dir = temp("read");
        write(&dir, &[("composer.json", COMPOSER), ("composer.lock", "{}"), ("vendor/bin/pint", ""), ("vendor/bin/phpunit", ""), ("vendor/bin/phpstan.bat", ""), (".php-cs-fixer.dist.php", ""), ("phpstan.neon", ""), ("artisan", "")]);
        let p = Project::read(&dir.to_string_lossy());
        assert!(p.composer && p.lock && p.artisan && p.phpstan_config);
        assert_eq!(p.bins.iter().map(String::as_str).collect::<Vec<_>>(), ["phpstan", "phpunit", "pint"], "Windows' .bat too");
        assert_eq!(p.cs_fixer_config.as_deref(), Some(".php-cs-fixer.dist.php"));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(Project::read(&dir.to_string_lossy()), Project::default(), "nothing there");
    }

    #[test]
    fn vendor_is_held_to_the_lockfile() {
        let lock = include_str!("samples/composer.lock");
        let installed = include_str!("samples/installed.json");
        assert_eq!(vendor_in_step(lock, installed), Ok(()));
        let older = installed.replace("\"version\": \"v11.6.1\"", "\"version\": \"v11.5.0\"");
        assert_eq!(vendor_in_step(lock, &older), Err("vendor differs from composer.lock: laravel/framework".into()));
        assert_eq!(vendor_in_step(lock, "{}"), Err("vendor differs from composer.lock: brick/math, laravel/framework, nesbot/carbon and 2 more".into()));
        let v1 = r#"[{"name": "brick/math", "version": "0.12.3"}]"#;
        assert_eq!(versions(v1), [("brick/math".to_string(), "0.12.3".to_string())], "Composer 1's installed.json");
    }
}
