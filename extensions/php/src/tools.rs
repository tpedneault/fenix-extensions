//! The programs the pack runs, found on this machine: PHP and Composer,
//! the language servers (Intelephense, which npm installs, and Phpactor,
//! on `PATH` or as the release the pack installs into its tools folder),
//! and the project's own tools in `vendor/bin` -- PHPUnit, Pest, PHPStan,
//! Pint and PHP CS Fixer -- looked at before `PATH` (`./vendor/bin`,
//! 0.8.7). `composer global require` and `npm install -g` put programs
//! in folders an editor started from the desktop may not have on its
//! `PATH`, and Laravel Herd puts PHP in its own, so they're looked in too.

use fenix_extension_api::Command;
use fenix_lang_kit::{Host, Install};

/// Where else to look: Composer's global folder, npm's, Herd's,
/// Homebrew's, and where PHP for Windows is usually unpacked.
pub const HINTS: &[&str] = &[
    "$HOME/.config/composer/vendor/bin", "$HOME/.composer/vendor/bin", "$HOME/.npm-global/bin", "$HOME/.volta/bin",
    "$HOME/Library/Application Support/Herd/bin", "$HOME/.config/herd-lite/bin", "/opt/homebrew/bin", "/usr/local/bin",
    "%APPDATA%\\Composer\\vendor\\bin", "%APPDATA%\\npm", "%USERPROFILE%\\.config\\herd\\bin", "C:\\php", "C:\\tools\\php",
];

/// The project's own folder of tools, looked at first.
pub const VENDOR_BIN: &str = "./vendor/bin";

/// Where `name` is looked for: the project's `vendor/bin` first for
/// its own tools, then [`HINTS`].
pub fn hints(name: &str) -> Vec<String> {
    let own = crate::project::BINS.contains(&name).then(|| VENDOR_BIN.to_string());
    own.into_iter().chain(HINTS.iter().map(|h| h.to_string())).collect()
}

/// What npm installs Intelephense from, and its program.
pub const INTELEPHENSE: &str = "intelephense";

/// Phpactor's release: a PHAR, run by PHP (8.2 or later).
pub const PHPACTOR: &str = "2026.10.07.0";
pub const PHPACTOR_SHA256: &str = "b28bf6166e016d539fc2f2aec18ef918b12590e436a6f1ccbdf30b5bbdbdb16b";

/// Phpactor's PHAR, into `/tools/phpactor`.
pub fn phpactor_install() -> Install {
    let url = format!("https://github.com/phpactor/phpactor/releases/download/{PHPACTOR}/phpactor.phar");
    Install::new("phpactor", &url).label(&format!("Phpactor {PHPACTOR}")).sha256(PHPACTOR_SHA256)
}

/// The language server to run (`php.server`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Server {
    #[default]
    Intelephense,
    Phpactor,
    Off,
}

impl Server {
    pub fn parse(text: &str) -> Server {
        match text.trim().to_ascii_lowercase().as_str() {
            "phpactor" => Server::Phpactor,
            "off" | "none" => Server::Off,
            _ => Server::Intelephense,
        }
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub php: Option<String>,
    pub composer: Option<String>,
    pub npm: Option<String>,
    pub intelephense: Option<String>,
    pub phpactor: Option<String>,
    /// Phpactor's PHAR, installed by the pack.
    pub phpactor_phar: bool,
    pub phpunit: Option<String>,
    pub pest: Option<String>,
    pub phpstan: Option<String>,
    pub pint: Option<String>,
    pub php_cs_fixer: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let mut find = |name: &str| host.find_program(name, &hints(name));
        Tools {
            php: find("php"),
            composer: find("composer"),
            npm: find("npm"),
            intelephense: find(INTELEPHENSE),
            phpactor: find("phpactor"),
            phpactor_phar: false,
            phpunit: find("phpunit"),
            pest: find("pest"),
            phpstan: find("phpstan"),
            pint: find("pint"),
            php_cs_fixer: find("php-cs-fixer"),
        }
    }

    /// A program by its path when found, else by its name.
    pub fn run(path: Option<&str>, name: &str) -> Command {
        Command::new(path.unwrap_or(name))
    }

    pub fn php(&self) -> Command {
        Tools::run(self.php.as_deref(), "php")
    }

    pub fn composer(&self) -> Command {
        Tools::run(self.composer.as_deref(), "composer")
    }

    /// `npm install -g intelephense`; none without npm.
    pub fn install_intelephense(&self) -> Option<Command> {
        self.npm.as_ref().map(|npm| Command::new(npm).args(["install", "-g", INTELEPHENSE]))
    }

    /// The server `wanted`, as found: the other one when it isn't, and
    /// none when neither is (or it's off). Phpactor on `PATH` before the
    /// pack's PHAR, which PHP runs.
    pub fn server(&self, wanted: Server) -> Option<Command> {
        let intelephense = self.intelephense.as_deref().map(|p| Command::new(p).arg("--stdio"));
        let phpactor = match (&self.phpactor, self.phpactor_phar) {
            (Some(p), _) => Some(Command::new(p).arg("language-server")),
            (None, true) => Some(self.php().args(["${tools}/phpactor/phpactor.phar", "language-server"])),
            (None, false) => None,
        };
        match wanted {
            Server::Intelephense => intelephense.or(phpactor),
            Server::Phpactor => phpactor.or(intelephense),
            Server::Off => None,
        }
    }

    /// The slot each program found goes in, by its name.
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<String>> {
        Some(match name {
            "php" => &mut self.php,
            "composer" => &mut self.composer,
            INTELEPHENSE => &mut self.intelephense,
            "phpactor" => &mut self.phpactor,
            "phpunit" => &mut self.phpunit,
            "pest" => &mut self.pest,
            "phpstan" => &mut self.phpstan,
            "pint" => &mut self.pint,
            "php-cs-fixer" => &mut self.php_cs_fixer,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;
    use fenix_lang_kit::host::mock::Mock;

    #[test]
    fn the_projects_own_tools_are_looked_for_in_vendor_bin_first() {
        assert_eq!(hints("pint")[0], "./vendor/bin");
        assert_eq!(hints("php")[0], "$HOME/.config/composer/vendor/bin", "PHP isn't the project's");
        let mut host = Mock::default();
        host.programs.extend([("php".to_string(), "/usr/bin/php".to_string()), ("pint".into(), "/p/vendor/bin/pint".into())]);
        let t = Tools::find(&mut host);
        assert_eq!((t.php.as_deref(), t.pint.as_deref(), t.composer.as_deref()), (Some("/usr/bin/php"), Some("/p/vendor/bin/pint"), None));
        assert_eq!(line(&t.composer()), "composer", "by its name when it isn't found");
        assert!(t.install_intelephense().is_none(), "no npm");
    }

    #[test]
    fn the_server_wanted_else_the_one_there_is() {
        let mut t = Tools { intelephense: Some("/n/intelephense".into()), npm: Some("/n/npm".into()), ..Tools::default() };
        assert_eq!(t.server(Server::Intelephense).map(|c| line(&c)).as_deref(), Some("/n/intelephense --stdio"));
        assert_eq!(t.server(Server::Phpactor).map(|c| line(&c)).as_deref(), Some("/n/intelephense --stdio"), "no Phpactor: Intelephense");
        assert_eq!(t.install_intelephense().map(|c| line(&c)).as_deref(), Some("/n/npm install -g intelephense"));
        t.phpactor_phar = true;
        t.php = Some("/usr/bin/php".into());
        assert_eq!(t.server(Server::Phpactor).map(|c| line(&c)).as_deref(), Some("/usr/bin/php ${tools}/phpactor/phpactor.phar language-server"));
        t.phpactor = Some("/h/phpactor".into());
        assert_eq!(t.server(Server::Phpactor).map(|c| line(&c)).as_deref(), Some("/h/phpactor language-server"), "on PATH before the PHAR");
        assert_eq!(t.server(Server::Off), None);
        assert_eq!(Tools::default().server(Server::Intelephense), None);
        assert_eq!((Server::parse(" Phpactor"), Server::parse("off"), Server::parse("")), (Server::Phpactor, Server::Off, Server::Intelephense));
    }

    #[test]
    fn phpactors_release_is_pinned() {
        let i = phpactor_install();
        assert_eq!(i.url, "https://github.com/phpactor/phpactor/releases/download/2026.10.07.0/phpactor.phar");
        assert_eq!((i.program.as_str(), i.sha256.as_deref()), ("phpactor.phar", Some(PHPACTOR_SHA256)));
        assert!(i.uses.is_empty(), "the pack sets it, with PHP");
    }
}
