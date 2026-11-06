//! The programs the pack runs, found on this machine: GHC, Cabal, Stack,
//! ghcup, the Haskell Language Server, hlint, ormolu and fourmolu.
//! ghcup, `cabal install`, `stack install`, Nix and Homebrew put them in
//! folders an editor started from the desktop may not have on its
//! `PATH`, so they're looked in too.

use fenix_extension_api::Command;
use fenix_lang_kit::Host;

/// Where else to look: ghcup's, Cabal's and Stack's bin folders, Nix's
/// profiles, Homebrew's; and ghcup's and Cabal's on Windows.
pub const HINTS: &[&str] = &[
    "$HOME/.ghcup/bin", "$HOME/.cabal/bin", "$HOME/.local/bin", "$HOME/.nix-profile/bin", "/nix/var/nix/profiles/default/bin",
    "/opt/homebrew/bin", "/usr/local/bin", "C:\\ghcup\\bin", "%APPDATA%\\cabal\\bin", "%APPDATA%\\local\\bin",
];

pub fn hints() -> Vec<String> {
    HINTS.iter().map(|h| h.to_string()).collect()
}

/// Where ghcup comes from: its page says how, for each OS.
pub const GHCUP_PAGE: &str = "https://www.haskell.org/ghcup/install/";

/// The formatter to use (`haskell.formatter`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Formatter {
    /// fourmolu in a project with a `fourmolu.yaml`, else ormolu (or
    /// fourmolu, when it's the only one installed).
    #[default]
    Auto,
    Ormolu,
    Fourmolu,
    /// The server's, if it formats.
    Off,
}

impl Formatter {
    pub fn parse(text: &str) -> Formatter {
        match text.trim().to_ascii_lowercase().as_str() {
            "ormolu" => Formatter::Ormolu,
            "fourmolu" => Formatter::Fourmolu,
            "off" => Formatter::Off,
            _ => Formatter::Auto,
        }
    }

    /// Its program, and HLS's name for it (`haskell.formattingProvider`).
    pub fn program(self) -> &'static str {
        match self {
            Formatter::Fourmolu => "fourmolu",
            Formatter::Off => "none",
            _ => "ormolu",
        }
    }
}

/// What builds the project (`haskell.build_tool`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BuildTool {
    /// Stack in a project with a `stack.yaml`, else Cabal.
    #[default]
    Auto,
    Cabal,
    Stack,
}

impl BuildTool {
    pub fn parse(text: &str) -> BuildTool {
        match text.trim().to_ascii_lowercase().as_str() {
            "cabal" => BuildTool::Cabal,
            "stack" => BuildTool::Stack,
            _ => BuildTool::Auto,
        }
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub ghc: Option<String>,
    pub runghc: Option<String>,
    pub cabal: Option<String>,
    pub stack: Option<String>,
    pub ghcup: Option<String>,
    /// `haskell-language-server-wrapper`, which starts the server built
    /// for the project's GHC.
    pub hls_wrapper: Option<String>,
    /// A server built for one GHC, as Nix and some systems name it.
    pub hls: Option<String>,
    pub hlint: Option<String>,
    pub ormolu: Option<String>,
    pub fourmolu: Option<String>,
}

/// The names the pack looks for, as `permissions.process` has them.
pub const PROGRAMS: &[&str] = &["ghc", "runghc", "cabal", "stack", "ghcup", "haskell-language-server-wrapper", "haskell-language-server", "hlint", "ormolu", "fourmolu"];

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let hints = hints();
        let mut t = Tools::default();
        for name in PROGRAMS {
            let found = host.find_program(name, &hints);
            if let Some(slot) = t.slot(name) {
                *slot = found;
            }
        }
        t
    }

    /// The slot each program found goes in, by its name.
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<String>> {
        Some(match name {
            "ghc" => &mut self.ghc,
            "runghc" => &mut self.runghc,
            "cabal" => &mut self.cabal,
            "stack" => &mut self.stack,
            "ghcup" => &mut self.ghcup,
            "haskell-language-server-wrapper" => &mut self.hls_wrapper,
            "haskell-language-server" => &mut self.hls,
            "hlint" => &mut self.hlint,
            "ormolu" => &mut self.ormolu,
            "fourmolu" => &mut self.fourmolu,
            _ => return None,
        })
    }

    /// Where `name` was found.
    pub fn found(&self, name: &str) -> Option<&str> {
        match name {
            "ghc" => self.ghc.as_deref(),
            "runghc" => self.runghc.as_deref(),
            "cabal" => self.cabal.as_deref(),
            "stack" => self.stack.as_deref(),
            "ghcup" => self.ghcup.as_deref(),
            "haskell-language-server-wrapper" => self.hls_wrapper.as_deref(),
            "haskell-language-server" => self.hls.as_deref(),
            "hlint" => self.hlint.as_deref(),
            "ormolu" => self.ormolu.as_deref(),
            "fourmolu" => self.fourmolu.as_deref(),
            _ => None,
        }
    }

    /// `name` by its path when found, else by its name.
    pub fn run(&self, name: &str) -> Command {
        Command::new(self.found(name).unwrap_or(name))
    }

    /// The language server: the wrapper, else a server of its own, each
    /// on stdio (`--lsp`).
    pub fn server(&self) -> Option<Command> {
        self.hls_wrapper.as_deref().or(self.hls.as_deref()).map(|p| Command::new(p).arg("--lsp"))
    }

    /// The formatter `wanted` is in `project`: `Auto` made one; none when
    /// it's off.
    pub fn formatter(&self, wanted: Formatter, fourmolu_yaml: bool) -> Formatter {
        match wanted {
            Formatter::Auto if fourmolu_yaml => Formatter::Fourmolu,
            Formatter::Auto if self.ormolu.is_none() && self.fourmolu.is_some() => Formatter::Fourmolu,
            Formatter::Auto => Formatter::Ormolu,
            other => other,
        }
    }

    /// What Fenix formats a Haskell buffer with: it on stdin, `${file}`
    /// its path, for the extensions its package's `.cabal` file turns on
    /// and fourmolu's config beside it.
    pub fn format_command(&self, which: Formatter) -> Option<Command> {
        let path = match which {
            Formatter::Ormolu => self.ormolu.as_deref()?,
            Formatter::Fourmolu => self.fourmolu.as_deref()?,
            _ => return None,
        };
        Some(Command::new(path).args(["--stdin-input-file", "${file}"]))
    }

    /// The build tool `wanted` is for a project with (or without) a
    /// `stack.yaml`.
    pub fn build_tool(wanted: BuildTool, stack_yaml: bool) -> BuildTool {
        match wanted {
            BuildTool::Auto if stack_yaml => BuildTool::Stack,
            BuildTool::Auto => BuildTool::Cabal,
            other => other,
        }
    }

    /// `ghcup install <tool> recommended`, when there's ghcup: what
    /// installs GHC, Cabal, Stack and HLS.
    pub fn ghcup_install(&self, tool: &str) -> Option<Command> {
        let ghcup = self.ghcup.as_deref()?;
        let cmd = Command::new(ghcup).args(["install", tool, "recommended"]);
        // GHC is made the one `ghc` runs, too.
        Some(if tool == "ghc" { Command::new(ghcup).args(["install", "ghc", "--set", "recommended"]) } else { cmd })
    }

    /// A package from Hackage, installed as a program: by Cabal, else by
    /// Stack (hlint, ormolu, fourmolu).
    pub fn hackage_install(&self, package: &str) -> Option<Command> {
        if let Some(cabal) = &self.cabal {
            return Some(Command::new(cabal).args(["install", package, "--install-method=copy", "--overwrite-policy=always"]));
        }
        self.stack.as_deref().map(|stack| Command::new(stack).args(["install", package]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    #[test]
    fn the_server_is_the_wrapper_else_a_server_of_its_own() {
        let mut t = Tools { hls: Some("/nix/hls".into()), ..Tools::default() };
        assert_eq!(t.server().map(|c| line(&c)).as_deref(), Some("/nix/hls --lsp"));
        t.hls_wrapper = Some("/g/haskell-language-server-wrapper".into());
        assert_eq!(t.server().map(|c| line(&c)).as_deref(), Some("/g/haskell-language-server-wrapper --lsp"));
        assert!(Tools::default().server().is_none());
        assert_eq!(t.run("cabal").program, "cabal");
        t.cabal = Some("/g/cabal".into());
        assert_eq!(t.run("cabal").program, "/g/cabal");
        assert!(t.slot("go").is_none());
    }

    #[test]
    fn auto_formats_with_fourmolu_where_its_configured() {
        let both = Tools { ormolu: Some("/g/ormolu".into()), fourmolu: Some("/g/fourmolu".into()), ..Tools::default() };
        assert_eq!(both.formatter(Formatter::Auto, false), Formatter::Ormolu);
        assert_eq!(both.formatter(Formatter::Auto, true), Formatter::Fourmolu);
        assert_eq!(both.formatter(Formatter::Ormolu, true), Formatter::Ormolu, "what's asked for wins");
        let only = Tools { fourmolu: Some("/g/fourmolu".into()), ..Tools::default() };
        assert_eq!(only.formatter(Formatter::Auto, false), Formatter::Fourmolu);
        assert_eq!(Tools::default().formatter(Formatter::Auto, false), Formatter::Ormolu, "the doctor asks for ormolu");
        assert_eq!(both.format_command(Formatter::Fourmolu).map(|c| line(&c)).as_deref(), Some("/g/fourmolu --stdin-input-file ${file}"));
        assert!(only.format_command(Formatter::Ormolu).is_none() && both.format_command(Formatter::Off).is_none());
        assert_eq!((Formatter::parse("Fourmolu"), Formatter::parse("off"), Formatter::parse("")), (Formatter::Fourmolu, Formatter::Off, Formatter::Auto));
    }

    #[test]
    fn ghcup_installs_the_toolchain_and_cabal_the_rest() {
        let t = Tools { ghcup: Some("/g/ghcup".into()), cabal: Some("/g/cabal".into()), ..Tools::default() };
        assert_eq!(t.ghcup_install("hls").map(|c| line(&c)).as_deref(), Some("/g/ghcup install hls recommended"));
        assert_eq!(t.ghcup_install("ghc").map(|c| line(&c)).as_deref(), Some("/g/ghcup install ghc --set recommended"));
        assert_eq!(t.hackage_install("hlint").map(|c| line(&c)).as_deref(), Some("/g/cabal install hlint --install-method=copy --overwrite-policy=always"));
        let stack = Tools { stack: Some("/g/stack".into()), ..Tools::default() };
        assert_eq!(stack.hackage_install("ormolu").map(|c| line(&c)).as_deref(), Some("/g/stack install ormolu"));
        assert!(Tools::default().ghcup_install("ghc").is_none() && Tools::default().hackage_install("hlint").is_none());
        assert_eq!(Tools::build_tool(BuildTool::Auto, true), BuildTool::Stack);
        assert_eq!(Tools::build_tool(BuildTool::Auto, false), BuildTool::Cabal);
        assert_eq!(Tools::build_tool(BuildTool::Cabal, true), BuildTool::Cabal);
        assert!(hints().contains(&"$HOME/.ghcup/bin".to_string()));
    }
}
