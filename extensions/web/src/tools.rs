//! The programs the pack runs, found on this machine: Node and the
//! package managers, corepack, fnm and Volta, the language servers, and
//! Prettier and Biome where the project has none of its own. `npm
//! install -g` puts programs in its prefix's folder, and pnpm, bun,
//! Volta and Yarn in theirs, which an editor started from the desktop may
//! not have on its `PATH`, so they're looked in too.

use fenix_extension_api::Command;
use fenix_lang_kit::Host;

use crate::project::Manager;

/// Where else to look. `${setting:web.node}` (0.8.5) is the Node you
/// chose, looked at first.
pub const HINTS: &[&str] = &[
    "$HOME/.npm-global/bin", "$HOME/.local/share/pnpm", "$HOME/Library/pnpm", "$HOME/.bun/bin", "$HOME/.volta/bin",
    "$HOME/.yarn/bin", "$HOME/.local/bin", "/opt/homebrew/bin", "/usr/local/bin",
    "%APPDATA%\\npm", "%LOCALAPPDATA%\\pnpm", "%USERPROFILE%\\.bun\\bin", "%LOCALAPPDATA%\\Volta\\bin", "%ProgramFiles%\\nodejs",
];

/// The setting naming the Node to use.
pub const NODE_SETTING: &str = "${setting:web.node}";

pub fn hints() -> Vec<String> {
    HINTS.iter().map(|h| h.to_string()).collect()
}

/// The language servers, by their programs.
pub const TSSERVER: &str = "typescript-language-server";
pub const ESLINT_SERVER: &str = "vscode-eslint-language-server";
pub const TAILWIND_SERVER: &str = "tailwindcss-language-server";
pub const CSS_SERVER: &str = "vscode-css-language-server";
pub const HTML_SERVER: &str = "vscode-html-language-server";

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub node: Option<String>,
    pub npm: Option<String>,
    pub pnpm: Option<String>,
    pub yarn: Option<String>,
    pub bun: Option<String>,
    pub corepack: Option<String>,
    pub fnm: Option<String>,
    pub volta: Option<String>,
    pub tsserver: Option<String>,
    pub eslint_server: Option<String>,
    pub tailwind_server: Option<String>,
    pub css_server: Option<String>,
    pub html_server: Option<String>,
    pub prettier: Option<String>,
    pub biome: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let hints = hints();
        let mut find = |name: &str| host.find_program(name, &hints);
        let mut t = Tools {
            npm: find("npm"),
            pnpm: find("pnpm"),
            yarn: find("yarn"),
            bun: find("bun"),
            corepack: find("corepack"),
            fnm: find("fnm"),
            volta: find("volta"),
            tsserver: find(TSSERVER),
            eslint_server: find(ESLINT_SERVER),
            tailwind_server: find(TAILWIND_SERVER),
            css_server: find(CSS_SERVER),
            html_server: find(HTML_SERVER),
            prettier: find("prettier"),
            biome: find("biome"),
            ..Tools::default()
        };
        let node_hints: Vec<String> = std::iter::once(NODE_SETTING.to_string()).chain(hints.iter().cloned()).collect();
        t.node = host.find_program("node", &node_hints);
        t
    }

    /// A program by its path when found, else by its name.
    pub fn run(path: Option<&str>, name: &str) -> Command {
        Command::new(path.unwrap_or(name))
    }

    pub fn node(&self) -> Command {
        Tools::run(self.node.as_deref(), "node")
    }

    pub fn npm(&self) -> Command {
        Tools::run(self.npm.as_deref(), "npm")
    }

    /// Where `m` was found.
    pub fn manager_path(&self, m: Manager) -> Option<&str> {
        match m {
            Manager::Npm => self.npm.as_deref(),
            Manager::Pnpm => self.pnpm.as_deref(),
            Manager::Yarn | Manager::Berry => self.yarn.as_deref(),
            Manager::Bun => self.bun.as_deref(),
        }
    }

    pub fn manager(&self, m: Manager) -> Command {
        Tools::run(self.manager_path(m), m.program())
    }

    /// Installing an npm package for you, not the project: `npm install
    /// -g`. None without npm.
    pub fn install_global(&self, packages: &[&str]) -> Option<Command> {
        self.npm.as_ref().map(|_| self.npm().args(["install", "-g"]).args(packages.iter().copied()))
    }

    /// Installing the Node the project asks for: fnm reads `.nvmrc` and
    /// `.node-version` itself (`pinned`); else fnm or Volta is given the
    /// version's first number (`>=20` is 20), or the LTS.
    pub fn install_node(&self, version: Option<&str>, pinned: bool) -> Option<Command> {
        let number = version.map(|v| v.trim_start_matches(|c: char| !c.is_ascii_digit())).and_then(|v| v.split(|c: char| !c.is_ascii_digit() && c != '.').next()).filter(|v| !v.is_empty());
        if let Some(fnm) = &self.fnm {
            let cmd = Command::new(fnm).arg("install");
            return Some(match number {
                _ if pinned => cmd,
                Some(n) => cmd.arg(n.split('.').next().unwrap_or(n)),
                None => cmd.arg("--lts"),
            });
        }
        let at = match number {
            Some(n) if pinned => n.to_string(),
            Some(n) => n.split('.').next().unwrap_or(n).to_string(),
            None => "lts".into(),
        };
        self.volta.as_ref().map(|v| Command::new(v).args(["install", &format!("node@{at}")]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;
    use fenix_lang_kit::host::mock::Mock;

    #[test]
    fn the_programs_are_found_node_by_its_setting_first() {
        let mut host = Mock::default();
        host.programs.extend([("node".to_string(), "/n/node".to_string()), ("pnpm".into(), "/n/pnpm".into()), (TSSERVER.into(), "/n/tsls".into())]);
        let t = Tools::find(&mut host);
        assert_eq!((t.node.as_deref(), t.pnpm.as_deref(), t.tsserver.as_deref(), t.npm.as_deref()), (Some("/n/node"), Some("/n/pnpm"), Some("/n/tsls"), None));
        assert_eq!(line(&t.manager(Manager::Berry)), "yarn", "by its name when it isn't found");
        assert_eq!(line(&t.manager(Manager::Pnpm)), "/n/pnpm");
        assert!(t.install_global(&["prettier"]).is_none(), "no npm");
        assert!(HINTS.contains(&"%APPDATA%\\npm") && NODE_SETTING == "${setting:web.node}");
    }

    #[test]
    fn node_is_installed_with_fnm_else_volta() {
        let mut t = Tools { npm: Some("/n/npm".into()), volta: Some("/v/volta".into()), ..Tools::default() };
        assert_eq!(t.install_global(&["typescript-language-server", "typescript"]).map(|c| line(&c)).as_deref(), Some("/n/npm install -g typescript-language-server typescript"));
        assert_eq!(t.install_node(Some("^20.11.0"), true).map(|c| line(&c)).as_deref(), Some("/v/volta install node@20.11.0"));
        assert_eq!(t.install_node(Some(">=20 <23"), false).map(|c| line(&c)).as_deref(), Some("/v/volta install node@20"));
        assert_eq!(t.install_node(None, false).map(|c| line(&c)).as_deref(), Some("/v/volta install node@lts"));
        t.fnm = Some("/f/fnm".into());
        assert_eq!(t.install_node(Some("^20.11.0"), true).map(|c| line(&c)).as_deref(), Some("/f/fnm install"), "fnm reads .nvmrc itself");
        assert_eq!(t.install_node(Some(">=18.17"), false).map(|c| line(&c)).as_deref(), Some("/f/fnm install 18"));
        assert_eq!(t.install_node(None, false).map(|c| line(&c)).as_deref(), Some("/f/fnm install --lts"));
        assert_eq!(Tools::default().install_node(Some("20"), true), None);
    }
}
