//! The programs the pack runs, found on this machine: uv, Python, ruff,
//! the language servers, pipx. uv's and pipx's tools go into
//! `~/.local/bin`, which an editor started from the desktop may not have
//! on its `PATH`, so it's looked in too.

use fenix_extension_api::Command;
use fenix_lang_kit::Host;

/// Where else to look: uv's, pipx's and pip `--user`'s folders.
pub const HINTS: &[&str] = &["$HOME/.local/bin", "$HOME/.cargo/bin", "%USERPROFILE%\\.local\\bin", "%APPDATA%\\Python\\Scripts"];

pub fn hints() -> Vec<String> {
    HINTS.iter().map(|h| h.to_string()).collect()
}

/// The Python you chose (`python.interpreter`), looked at first: a hint
/// `find-program` reads as that setting's path (0.8.5).
pub const INTERPRETER: &str = "${setting:python.interpreter}";

/// Where to look for Python: the one you chose, then the others.
pub fn python_hints() -> Vec<String> {
    std::iter::once(INTERPRETER.to_string()).chain(hints()).collect()
}

/// The language server to prefer (`python.server`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Server {
    #[default]
    Basedpyright,
    Pyright,
}

impl Server {
    pub fn parse(text: &str) -> Server {
        if text.trim().eq_ignore_ascii_case("pyright") {
            Server::Pyright
        } else {
            Server::Basedpyright
        }
    }

    /// Its package: what's installed.
    pub fn package(self) -> &'static str {
        match self {
            Server::Basedpyright => "basedpyright",
            Server::Pyright => "pyright",
        }
    }

    /// Its server's program.
    pub fn program(self) -> &'static str {
        match self {
            Server::Basedpyright => "basedpyright-langserver",
            Server::Pyright => "pyright-langserver",
        }
    }

    pub fn other(self) -> Server {
        match self {
            Server::Basedpyright => Server::Pyright,
            Server::Pyright => Server::Basedpyright,
        }
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub uv: Option<String>,
    /// `python3`, or `python` where that's its name (Windows).
    pub python: Option<String>,
    pub ruff: Option<String>,
    pub basedpyright: Option<String>,
    pub pyright: Option<String>,
    pub pipx: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let hints = hints();
        let python_hints = python_hints();
        let python = host.find_program("python3", &python_hints).or_else(|| host.find_program("python", &python_hints));
        let mut find = |name: &str| host.find_program(name, &hints);
        Tools {
            uv: find("uv"),
            python,
            ruff: find("ruff"),
            basedpyright: find(Server::Basedpyright.program()),
            pyright: find(Server::Pyright.program()),
            pipx: find("pipx"),
        }
    }

    pub fn server(&self, s: Server) -> Option<&str> {
        match s {
            Server::Basedpyright => self.basedpyright.as_deref(),
            Server::Pyright => self.pyright.as_deref(),
        }
    }

    /// The server to use: the one asked for, else the other, when found.
    pub fn pick_server(&self, wanted: Server) -> Option<(Server, &str)> {
        [wanted, wanted.other()].into_iter().find_map(|s| self.server(s).map(|p| (s, p)))
    }

    /// A program by its path when found, else by its name.
    pub fn run(path: Option<&str>, name: &str) -> Command {
        Command::new(path.unwrap_or(name))
    }

    pub fn uv(&self) -> Command {
        Tools::run(self.uv.as_deref(), "uv")
    }

    pub fn python(&self) -> Command {
        Tools::run(self.python.as_deref(), "python3")
    }

    /// Installing Python tool `package` for you, not the project: `uv tool
    /// install`, `pipx install`, or pip's `--user`.
    pub fn install_tool(&self, package: &str) -> Option<Command> {
        if self.uv.is_some() {
            Some(self.uv().args(["tool", "install", package]))
        } else if let Some(pipx) = &self.pipx {
            Some(Command::new(pipx).args(["install", package]))
        } else {
            self.python.as_ref().map(|_| self.python().args(["-m", "pip", "install", "--user", package]))
        }
    }

    /// Installing uv itself.
    pub fn install_uv(&self) -> Option<Command> {
        Tools { uv: None, ..self.clone() }.install_tool("uv")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    #[test]
    fn a_tool_is_installed_with_uv_then_pipx_then_pip() {
        let mut t = Tools { uv: Some("/u/uv".into()), pipx: Some("/u/pipx".into()), python: Some("/usr/bin/python3".into()), ..Tools::default() };
        assert_eq!(t.install_tool("ruff").map(|c| line(&c)).as_deref(), Some("/u/uv tool install ruff"));
        assert_eq!(t.install_uv().map(|c| line(&c)).as_deref(), Some("/u/pipx install uv"));
        t.pipx = None;
        assert_eq!(t.install_uv().map(|c| line(&c)).as_deref(), Some("/usr/bin/python3 -m pip install --user uv"));
        t.python = None;
        assert_eq!(t.install_uv(), None, "nothing to install it with");
    }

    #[test]
    fn the_server_asked_for_is_used_or_else_the_other() {
        let t = Tools { pyright: Some("/p".into()), ..Tools::default() };
        assert_eq!(t.pick_server(Server::Basedpyright), Some((Server::Pyright, "/p")));
        assert_eq!(Tools::default().pick_server(Server::Pyright), None);
        assert_eq!(Server::parse(" Pyright"), Server::Pyright);
        assert_eq!(Server::parse(""), Server::Basedpyright);
    }
}
