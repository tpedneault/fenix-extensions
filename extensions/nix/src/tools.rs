//! The programs the pack runs, found on this machine. Nix's own tools come
//! from Nix: a profile (`~/.nix-profile`, `nix profile`'s new home,
//! NixOS's system and per-user profiles), the project's dev shell, or a
//! package manager -- not from a release the pack could download -- so it
//! looks where those put them, not only on `PATH`: an editor started from
//! the desktop may not have the profile's folder. Nothing is installed.

use fenix_extension_api::Command;
use fenix_lang_kit::Host;

/// Where else to look: the profiles Nix keeps (the classic one, `nix
/// profile`'s under `$XDG_STATE_HOME`, `/nix/profile`, a NixOS system's and
/// the user's own, the multi-user default), then the user's own bin folders
/// and Homebrew's.
pub const HINTS: &[&str] = &[
    "$HOME/.nix-profile/bin",
    "$HOME/.local/state/nix/profile/bin",
    "/nix/profile/bin",
    "/etc/profiles/per-user/$USER/bin",
    "/run/current-system/sw/bin",
    "/nix/var/nix/profiles/default/bin",
    "$HOME/.local/bin",
    "$HOME/bin",
    "$HOME/.cargo/bin",
    "/opt/homebrew/bin",
    "/usr/local/bin",
];

/// `name`'s hints.
pub fn hints() -> Vec<String> {
    HINTS.iter().map(|h| h.to_string()).collect()
}

/// Every program the pack finds, by the name `permissions.process` gives it.
pub const NAMES: &[&str] = &["nix", "nix-build", "nix-shell", "nixos-rebuild", "home-manager", "nixd", "nil", "nixfmt", "alejandra", "nixpkgs-fmt", "statix", "deadnix"];

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub nix: Option<String>,
    pub nix_build: Option<String>,
    pub nix_shell: Option<String>,
    pub nixos_rebuild: Option<String>,
    pub home_manager: Option<String>,
    pub nixd: Option<String>,
    pub nil: Option<String>,
    pub nixfmt: Option<String>,
    pub alejandra: Option<String>,
    pub nixpkgs_fmt: Option<String>,
    pub statix: Option<String>,
    pub deadnix: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let mut t = Tools::default();
        for name in NAMES {
            let found = host.find_program(name, &hints());
            if let Some(slot) = t.slot(name) {
                *slot = found;
            }
        }
        t
    }

    /// The slot each program found goes in, by its name.
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<String>> {
        Some(match name {
            "nix" => &mut self.nix,
            "nix-build" => &mut self.nix_build,
            "nix-shell" => &mut self.nix_shell,
            "nixos-rebuild" => &mut self.nixos_rebuild,
            "home-manager" => &mut self.home_manager,
            "nixd" => &mut self.nixd,
            "nil" => &mut self.nil,
            "nixfmt" => &mut self.nixfmt,
            "alejandra" => &mut self.alejandra,
            "nixpkgs-fmt" => &mut self.nixpkgs_fmt,
            "statix" => &mut self.statix,
            "deadnix" => &mut self.deadnix,
            _ => return None,
        })
    }

    /// Where `name` was found.
    pub fn get(&self, name: &str) -> Option<&str> {
        match name {
            "nix" => self.nix.as_deref(),
            "nix-build" => self.nix_build.as_deref(),
            "nix-shell" => self.nix_shell.as_deref(),
            "nixos-rebuild" => self.nixos_rebuild.as_deref(),
            "home-manager" => self.home_manager.as_deref(),
            "nixd" => self.nixd.as_deref(),
            "nil" => self.nil.as_deref(),
            "nixfmt" => self.nixfmt.as_deref(),
            "alejandra" => self.alejandra.as_deref(),
            "nixpkgs-fmt" => self.nixpkgs_fmt.as_deref(),
            "statix" => self.statix.as_deref(),
            "deadnix" => self.deadnix.as_deref(),
            _ => None,
        }
    }

    /// `name` by its path when found, else by its name.
    pub fn command(&self, name: &str) -> Command {
        Command::new(self.get(name).unwrap_or(name))
    }
}

/// Nix's name for this machine (`x86_64-linux`), as `platform()` says its
/// OS and architecture; `None` for one Nix has no name for.
pub fn system(os: &str, arch: &str) -> Option<String> {
    let arch = match arch {
        "x86_64" | "aarch64" | "riscv64" | "i686" => arch,
        "arm64" => "aarch64",
        "x86" => "i686",
        _ => return None,
    };
    let os = match os {
        "linux" => "linux",
        "macos" => "darwin",
        _ => return None,
    };
    Some(format!("{arch}-{os}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profiles_are_looked_in() {
        let h = hints();
        for want in ["$HOME/.nix-profile/bin", "$HOME/.local/state/nix/profile/bin", "/etc/profiles/per-user/$USER/bin", "/run/current-system/sw/bin", "/nix/var/nix/profiles/default/bin"] {
            assert!(h.contains(&want.to_string()), "{want}");
        }
        assert!(!h.iter().any(|h| h.starts_with("/tools")), "nothing is installed into the tools folder");
    }

    #[test]
    fn every_program_has_a_slot_and_is_got_by_name() {
        let mut t = Tools::default();
        for name in NAMES {
            assert!(t.slot(name).is_some(), "{name}");
            *t.slot(name).unwrap() = Some(format!("/n/{name}"));
        }
        for name in NAMES {
            assert_eq!(t.get(name), Some(format!("/n/{name}").as_str()), "{name}");
        }
        assert!(t.slot("go").is_none() && t.get("go").is_none());
        assert_eq!(Tools::default().command("nix").program, "nix");
        assert_eq!(t.command("statix").program, "/n/statix");
    }

    #[test]
    fn nixs_names_for_machines() {
        assert_eq!(system("linux", "x86_64").as_deref(), Some("x86_64-linux"));
        assert_eq!(system("linux", "aarch64").as_deref(), Some("aarch64-linux"));
        assert_eq!(system("macos", "arm64").as_deref(), Some("aarch64-darwin"));
        assert_eq!(system("macos", "x86_64").as_deref(), Some("x86_64-darwin"));
        assert_eq!(system("windows", "x86_64"), None);
    }
}
