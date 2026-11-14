//! A flake's outputs as `nix flake show --json` lists them, for the pickers
//! of what to build, run or enter. Nix isn't asked more than that one
//! command: what it prints is read, and nothing is evaluated by the pack.
//!
//! The JSON is an object of output types -- `packages`, `apps`, `checks`,
//! `devShells` by system, `nixosConfigurations`, `homeConfigurations` by
//! name -- each entry an object (`"type": "derivation"`, a `name`, a
//! `description`). An output that isn't in that shape is passed over.

use fenix_lang_kit::serde_json::Value;

/// What the picker is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Build,
    Run,
    Develop,
}

impl Action {
    /// What the picker says it's choosing.
    pub fn title(self) -> &'static str {
        match self {
            Action::Build => "Flake output to build",
            Action::Run => "Flake output to run",
            Action::Develop => "Dev shell to enter",
        }
    }
}

/// One output to pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `packages.default`.
    pub label: String,
    /// Its description, or the derivation's name.
    pub detail: String,
    /// What `nix` is given: `.#default`.
    pub installable: String,
}

/// `name` as one part of an attribute path: quoted when it holds anything
/// but letters, digits, `_`, `-` and `'` (`"me@station"`).
pub fn attr(name: &str) -> String {
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '\'')) {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// The entries of `section` that are for `system` -- all of them, merged, if
/// the machine has no name Nix knows -- as (name, entry), sorted.
fn per_system<'a>(json: &'a Value, section: &str, system: Option<&str>) -> Vec<(&'a str, &'a Value)> {
    let Some(systems) = json.get(section).and_then(Value::as_object) else { return Vec::new() };
    let mut out: Vec<(&str, &Value)> = Vec::new();
    for (sys, entries) in systems {
        if system.is_some_and(|s| s != sys) {
            continue;
        }
        let Some(entries) = entries.as_object() else { continue };
        for (name, entry) in entries {
            if entry.is_object() && !out.iter().any(|(n, _)| n == name) {
                out.push((name, entry));
            }
        }
    }
    out.sort_by_key(|(n, _)| *n);
    out
}

fn by_name<'a>(json: &'a Value, section: &str) -> Vec<(&'a str, &'a Value)> {
    json.get(section).and_then(Value::as_object).map(|m| m.iter().filter(|(_, v)| v.is_object()).map(|(k, v)| (k.as_str(), v)).collect()).unwrap_or_default()
}

fn detail(entry: &Value) -> String {
    ["description", "name"].iter().find_map(|k| entry.get(k).and_then(Value::as_str).filter(|s| !s.is_empty())).unwrap_or("").to_string()
}

/// The outputs to pick from for `action`, on `system` (`x86_64-linux`):
///
/// - build: `packages`, then `checks`, the dev shells, and the NixOS and
///   home-manager configurations' system and activation packages;
/// - run: `apps`, then `packages` (`nix run` runs a package's main program);
/// - develop: `devShells`.
pub fn targets(json: &Value, system: Option<&str>, action: Action) -> Vec<Target> {
    let mut out = Vec::new();
    let mut add = |label: String, entry: &Value, installable: String| out.push(Target { label, detail: detail(entry), installable });
    match action {
        Action::Build => {
            for (name, e) in per_system(json, "packages", system) {
                add(format!("packages.{name}"), e, format!(".#{}", attr(name)));
            }
            let at = system.map(str::to_string);
            for (section, label) in [("checks", "checks"), ("devShells", "devShells")] {
                for (name, e) in per_system(json, section, system) {
                    // Without a system Nix knows, there's no honest path to give.
                    if let Some(s) = &at {
                        add(format!("{label}.{name}"), e, format!(".#{section}.{s}.{}", attr(name)));
                    }
                }
            }
            for (name, e) in by_name(json, "nixosConfigurations") {
                add(format!("nixosConfigurations.{name}"), e, format!(".#nixosConfigurations.{}.config.system.build.toplevel", attr(name)));
            }
            for (name, e) in by_name(json, "homeConfigurations") {
                add(format!("homeConfigurations.{name}"), e, format!(".#homeConfigurations.{}.activationPackage", attr(name)));
            }
        }
        Action::Run => {
            for (name, e) in per_system(json, "apps", system) {
                add(format!("apps.{name}"), e, format!(".#{}", attr(name)));
            }
            let apps: Vec<String> = per_system(json, "apps", system).into_iter().map(|(n, _)| n.to_string()).collect();
            for (name, e) in per_system(json, "packages", system) {
                if !apps.iter().any(|a| a == name) {
                    add(format!("packages.{name}"), e, format!(".#{}", attr(name)));
                }
            }
        }
        Action::Develop => {
            for (name, e) in per_system(json, "devShells", system) {
                let installable = if name == "default" { ".".to_string() } else { format!(".#{}", attr(name)) };
                add(format!("devShells.{name}"), e, installable);
            }
        }
    }
    out
}

/// What `nix flake show --json` printed, as JSON; or why it can't be read.
/// Nix prints its progress and warnings on stderr, so stdout is the JSON.
pub fn read(stdout: &str) -> Result<Value, String> {
    let json: Value = fenix_lang_kit::serde_json::from_str(stdout.trim()).map_err(|e| format!("nix flake show --json printed something else: {e}"))?;
    if json.is_object() {
        Ok(json)
    } else {
        Err("nix flake show --json printed something else than an object".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        read(include_str!("samples/flake-show.json")).unwrap()
    }

    fn rows(t: &[Target]) -> Vec<(&str, &str)> {
        t.iter().map(|t| (t.label.as_str(), t.installable.as_str())).collect()
    }

    #[test]
    fn what_can_be_built() {
        let t = targets(&sample(), Some("x86_64-linux"), Action::Build);
        assert_eq!(
            rows(&t),
            [
                ("packages.default", ".#default"),
                ("packages.docs", ".#docs"),
                ("checks.fmt", ".#checks.x86_64-linux.fmt"),
                ("devShells.ci", ".#devShells.x86_64-linux.ci"),
                ("devShells.default", ".#devShells.x86_64-linux.default"),
                ("nixosConfigurations.station", ".#nixosConfigurations.station.config.system.build.toplevel"),
                ("homeConfigurations.me@station", ".#homeConfigurations.\"me@station\".activationPackage"),
            ]
        );
        assert_eq!(t[0].detail, "Ground station tools", "a description when there is one");
        assert_eq!(t[1].detail, "orbit-docs-1.0", "else the derivation's name");
    }

    #[test]
    fn what_can_be_run_apps_first() {
        let t = targets(&sample(), Some("x86_64-linux"), Action::Run);
        assert_eq!(rows(&t), [("apps.serve", ".#serve"), ("packages.default", ".#default"), ("packages.docs", ".#docs")]);
    }

    #[test]
    fn the_default_dev_shell_is_the_bare_flake() {
        let t = targets(&sample(), Some("x86_64-linux"), Action::Develop);
        assert_eq!(rows(&t), [("devShells.ci", ".#ci"), ("devShells.default", ".")]);
    }

    #[test]
    fn only_this_machines_outputs_are_offered() {
        let arm = targets(&sample(), Some("aarch64-linux"), Action::Build);
        assert_eq!(rows(&arm)[0], ("packages.arm-only", ".#arm-only"));
        assert!(!arm.iter().any(|t| t.label == "packages.default"));
        // A machine Nix has no name for: every system's packages, and no path to a check.
        let any = targets(&sample(), None, Action::Build);
        assert!(any.iter().any(|t| t.label == "packages.arm-only") && any.iter().any(|t| t.label == "packages.default"));
        assert!(!any.iter().any(|t| t.label.starts_with("checks.")));
    }

    #[test]
    fn what_isnt_in_the_shape_is_passed_over() {
        let odd = read(r#"{"packages": {"x86_64-linux": {"a": 1, "b": {"type": "derivation"}}}, "apps": [], "legacyPackages": {"x86_64-linux": {"omitted": true}}, "nixosConfigurations": 3}"#).unwrap();
        assert_eq!(rows(&targets(&odd, Some("x86_64-linux"), Action::Build)), [("packages.b", ".#b")]);
        assert!(targets(&odd, Some("x86_64-linux"), Action::Develop).is_empty());
        assert!(read("not json").is_err() && read("[1]").is_err() && read("").is_err());
    }

    #[test]
    fn attribute_names_are_quoted_when_they_must_be() {
        assert_eq!(attr("default"), "default");
        assert_eq!(attr("my-pkg_2'"), "my-pkg_2'");
        assert_eq!(attr("me@host"), "\"me@host\"");
        assert_eq!(attr("a.b"), "\"a.b\"");
        assert_eq!(attr("q\"x"), "\"q\\\"x\"");
    }
}
