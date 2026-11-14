//! What the pack knows of the project, from its files: whether it's a
//! flake, a plain `default.nix` or `shell.nix` one, a NixOS or home-manager
//! configuration, and which formatter the flake says it uses. Nothing is
//! evaluated -- Nix isn't run to find out -- so what's read is the flake's
//! text, and a guess is a guess: the doctor says what it went by.

use std::path::Path;

/// The files that say something about a project, read at its top.
pub const FILES: &[&str] = &["flake.nix", "flake.lock", "default.nix", "shell.nix", "configuration.nix", "home.nix", "treefmt.toml", ".treefmt.toml", "treefmt.nix"];

/// The most of `flake.nix` that's read.
const FLAKE_LIMIT: u64 = 512 * 1024;

/// A formatter Fenix can run directly on a buffer's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Formatter {
    /// The official one (`nixfmt`, once `nixfmt-rfc-style`).
    Nixfmt,
    Alejandra,
    NixpkgsFmt,
}

impl Formatter {
    pub const ALL: [Formatter; 3] = [Formatter::Nixfmt, Formatter::Alejandra, Formatter::NixpkgsFmt];

    /// The program's name.
    pub fn name(self) -> &'static str {
        match self {
            Formatter::Nixfmt => "nixfmt",
            Formatter::Alejandra => "alejandra",
            Formatter::NixpkgsFmt => "nixpkgs-fmt",
        }
    }

    pub fn from_name(name: &str) -> Option<Formatter> {
        Formatter::ALL.into_iter().find(|f| f.name() == name)
    }

    /// What it's run with to format its stdin onto its stdout.
    pub fn args(self) -> &'static [&'static str] {
        match self {
            // No file given: it reads stdin.
            Formatter::Nixfmt => &[],
            // `-q` keeps its "Formatting stdin" note off stderr; `-` is stdin.
            Formatter::Alejandra => &["--quiet", "-"],
            // No file given: it reads stdin.
            Formatter::NixpkgsFmt => &[],
        }
    }
}

/// What the project's files say its formatter is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    /// One of the three, which the flake's `formatter` output names.
    Tool(Formatter),
    /// treefmt: a wrapper over several, with its own configuration, that
    /// a direct call can't stand in for.
    Treefmt,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    pub flake: bool,
    pub lock: bool,
    pub default_nix: bool,
    pub shell_nix: bool,
    /// `nixosConfigurations` in the flake, or a `configuration.nix`.
    pub nixos: bool,
    /// `homeConfigurations` in the flake, or a `home.nix`.
    pub home_manager: bool,
    pub configuration_nix: bool,
    pub home_nix: bool,
    /// The flake has a `formatter` output, so `nix fmt` works.
    pub flake_formatter: bool,
    pub hint: Option<Hint>,
}

impl Project {
    pub fn read(root: &str) -> Project {
        let root = Path::new(root);
        let has = |name: &str| root.join(name).is_file();
        let mut p = Project {
            flake: has("flake.nix"),
            lock: has("flake.lock"),
            default_nix: has("default.nix"),
            shell_nix: has("shell.nix"),
            configuration_nix: has("configuration.nix"),
            home_nix: has("home.nix"),
            ..Project::default()
        };
        let text = if p.flake { read_limited(&root.join("flake.nix")) } else { String::new() };
        let scan = scan_flake(&text);
        p.flake_formatter = scan.formatter_output;
        p.hint = scan.hint.or_else(|| (has("treefmt.toml") || has(".treefmt.toml") || has("treefmt.nix")).then_some(Hint::Treefmt));
        p.nixos = scan.nixos || p.configuration_nix;
        p.home_manager = scan.home_manager || p.home_nix;
        p
    }

    /// A Nix project: something to build or enter. A lone
    /// `configuration.nix` or `home.nix` is a configuration, not one.
    pub fn is_nix(&self) -> bool {
        self.flake || self.default_nix || self.shell_nix
    }

    /// A dev shell to enter: the flake's, or `shell.nix`.
    pub fn has_shell(&self) -> bool {
        self.flake || self.shell_nix
    }
}

fn read_limited(path: &Path) -> String {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else { return String::new() };
    let mut bytes = Vec::new();
    let _ = file.take(FLAKE_LIMIT).read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

/// What a flake's text says.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Scan {
    pub formatter_output: bool,
    pub hint: Option<Hint>,
    pub nixos: bool,
    pub home_manager: bool,
}

/// `text` with its `#` comments taken out.
fn uncommented(text: &str) -> String {
    text.lines().map(|l| l.split('#').next().unwrap_or("")).collect::<Vec<_>>().join("\n")
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).filter(|w| !w.is_empty())
}

fn hint_of(word: &str) -> Option<Hint> {
    match word {
        "nixfmt" | "nixfmt-rfc-style" | "nixfmt-classic" => Some(Hint::Tool(Formatter::Nixfmt)),
        "alejandra" => Some(Hint::Tool(Formatter::Alejandra)),
        "nixpkgs-fmt" => Some(Hint::Tool(Formatter::NixpkgsFmt)),
        w if w.starts_with("treefmt") => Some(Hint::Treefmt),
        _ => None,
    }
}

/// A flake's text read for its `formatter` output -- `formatter.${system} =
/// pkgs.alejandra;`, `formatter = forAllSystems (...)`: the statement up to
/// its `;` names the tool -- and for the configurations it declares.
pub fn scan_flake(text: &str) -> Scan {
    let code = uncommented(text);
    let mut scan = Scan { nixos: words(&code).any(|w| w == "nixosConfigurations"), home_manager: words(&code).any(|w| w == "homeConfigurations"), ..Scan::default() };
    let mut from = 0;
    while let Some(at) = code[from..].find("formatter") {
        let start = from + at;
        let end = start + "formatter".len();
        from = end;
        let before = code[..start].chars().next_back();
        let after = code[end..].chars().next();
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.') || after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            continue;
        }
        let statement = code[end..].split(';').next().unwrap_or("");
        // An assignment to the attribute: `formatter.x = ...` or `formatter = ...`.
        let Some((target, value)) = statement.split_once('=') else { continue };
        if target.trim().contains(char::is_whitespace) && !target.trim_start().starts_with('.') {
            continue;
        }
        scan.formatter_output = true;
        if scan.hint.is_none() {
            scan.hint = words(value).find_map(hint_of);
        }
    }
    scan
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh folder of this test's own, under the system's temporary one.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-nix-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_flakes_formatter_output_names_the_tool() {
        let sample = scan_flake(include_str!("samples/flake.nix"));
        assert_eq!((sample.formatter_output, sample.hint), (true, Some(Hint::Tool(Formatter::Nixfmt))));
        assert!(!sample.nixos && !sample.home_manager);
        for (text, hint) in [
            ("formatter.x86_64-linux = nixpkgs.legacyPackages.x86_64-linux.alejandra;", Some(Hint::Tool(Formatter::Alejandra))),
            ("formatter = forAllSystems (system: nixpkgs.legacyPackages.${system}.nixpkgs-fmt);", Some(Hint::Tool(Formatter::NixpkgsFmt))),
            ("formatter.${system} =\n  pkgs.nixfmt-classic;", Some(Hint::Tool(Formatter::Nixfmt))),
            ("formatter = treefmtEval.config.build.wrapper;", Some(Hint::Treefmt)),
            ("formatter = pkgs.writeShellScriptBin \"fmt\" \"true\";", None),
        ] {
            let s = scan_flake(text);
            assert_eq!((s.formatter_output, s.hint), (true, hint), "{text}");
        }
    }

    #[test]
    fn only_the_formatter_output_counts_not_a_word_that_has_it_in_it() {
        // A tool in the dev shell is not the project's formatter.
        let s = scan_flake("devShells.default = pkgs.mkShell { packages = [ pkgs.alejandra ]; };\nchecks.x = 1;");
        assert_eq!((s.formatter_output, s.hint), (false, None));
        let s = scan_flake("fmt.formatter-name = 1; my-formatter = alejandra; formatters = 2;");
        assert!(!s.formatter_output);
        // A comment isn't a formatter.
        let s = scan_flake("# formatter = alejandra;\n{ }");
        assert!(!s.formatter_output && s.hint.is_none());
        // `nix fmt` with a formatter that names no known tool still works.
        assert!(scan_flake("formatter.x = something;").formatter_output);
        // Only the first statement's tool: a later one is not the formatter.
        let s = scan_flake("formatter.a = pkgs.alejandra;\nformatter.b = pkgs.nixfmt;");
        assert_eq!(s.hint, Some(Hint::Tool(Formatter::Alejandra)));
    }

    #[test]
    fn a_flake_declares_its_configurations() {
        let s = scan_flake("outputs = { ... }: { nixosConfigurations.host = nixpkgs.lib.nixosSystem { }; homeConfigurations.\"me@host\" = 1; };");
        assert!(s.nixos && s.home_manager);
        let s = scan_flake("# nixosConfigurations.host\n{}");
        assert!(!s.nixos);
    }

    #[test]
    fn the_projects_files_are_read() {
        let dir = temp("project");
        assert_eq!(Project::read(&dir.to_string_lossy()), Project::default());
        assert!(!Project::default().is_nix());
        std::fs::write(dir.join("flake.nix"), include_str!("samples/flake.nix")).unwrap();
        std::fs::write(dir.join("flake.lock"), "{}").unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert!(p.flake && p.lock && p.is_nix() && p.has_shell() && p.flake_formatter && !p.default_nix && !p.nixos);
        assert_eq!(p.hint, Some(Hint::Tool(Formatter::Nixfmt)));
        std::fs::remove_dir_all(&dir).unwrap();

        let dir = temp("project-plain");
        std::fs::write(dir.join("default.nix"), "{ }").unwrap();
        std::fs::write(dir.join("treefmt.toml"), "").unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert!(p.is_nix() && p.default_nix && !p.has_shell() && !p.flake);
        assert_eq!(p.hint, Some(Hint::Treefmt), "a treefmt.toml beside no flake output");
        std::fs::remove_dir_all(&dir).unwrap();

        let dir = temp("project-config");
        std::fs::write(dir.join("configuration.nix"), "{ }").unwrap();
        std::fs::write(dir.join("home.nix"), "{ }").unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert!(!p.is_nix() && p.nixos && p.home_manager, "a configuration is not a project to build");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_formatters_read_stdin() {
        assert_eq!(Formatter::from_name("alejandra"), Some(Formatter::Alejandra));
        assert_eq!(Formatter::from_name("treefmt"), None);
        assert_eq!(Formatter::Alejandra.args(), ["--quiet", "-"]);
        assert!(Formatter::Nixfmt.args().is_empty() && Formatter::NixpkgsFmt.args().is_empty());
    }
}
