//! What the pack knows of the project, read from its files: `mix.exs`
//! (the dependencies it names, the Elixir it asks for), `.formatter.exs`
//! (whether Phoenix's HEEx formatter is a plugin of it) and `.credo.exs`.
//! They're read as text, not run: `mix.exs` is Elixir, and running it
//! would mean starting the VM for what a `{:phoenix, ...}` line says.

use std::path::Path;

/// A Mix project, as far as the pack needs to tell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// There's a `mix.exs` at the top.
    pub mix: bool,
    /// An umbrella: `apps_path` in `mix.exs`.
    pub umbrella: bool,
    /// `:phoenix` is a dependency: `mix phx.server` is a task.
    pub phoenix: bool,
    /// `:phoenix_live_view` is one.
    pub live_view: bool,
    /// `:credo` is a dependency, or there's a `.credo.exs`.
    pub credo: bool,
    /// `.formatter.exs` lists `Phoenix.LiveView.HTMLFormatter` as a plugin:
    /// `mix format` can format `.heex`.
    pub heex_formatter: bool,
    /// The Elixir `mix.exs` asks for (`elixir: "~> 1.15"`): its version.
    pub elixir: Option<String>,
}

impl Project {
    /// The project in `root`.
    pub fn read(root: &str) -> Project {
        let root = Path::new(root);
        let text = |name: &str| std::fs::read_to_string(root.join(name)).ok();
        let mix = text("mix.exs");
        let formatter = text(".formatter.exs").unwrap_or_default();
        let mix_text = mix.clone().unwrap_or_default();
        let credo_file = root.join(".credo.exs").is_file();
        Project {
            mix: mix.is_some(),
            umbrella: has_key(&mix_text, "apps_path"),
            phoenix: depends_on(&mix_text, "phoenix"),
            live_view: depends_on(&mix_text, "phoenix_live_view"),
            credo: depends_on(&mix_text, "credo") || credo_file,
            heex_formatter: formatter.contains("Phoenix.LiveView.HTMLFormatter"),
            elixir: requirement(&mix_text).and_then(|r| minimum(&r)),
        }
    }

    /// Whether this is a Mix project at all.
    pub fn is_elixir(&self) -> bool {
        self.mix
    }
}

/// Source with its comments taken out: `# {:phoenix, ...}` is nothing.
fn code(text: &str) -> String {
    text.lines().map(|l| l.split_once('#').filter(|(before, _)| before.matches('"').count() % 2 == 0).map_or(l, |(before, _)| before)).collect::<Vec<_>>().join("\n")
}

/// Whether `{:name,` is in the code: a dependency's tuple.
fn depends_on(text: &str, name: &str) -> bool {
    let code = code(text);
    let mut rest = code.as_str();
    while let Some(at) = rest.find("{:") {
        let after = &rest[at + 2..];
        let end = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(after.len());
        if &after[..end] == name && after[end..].trim_start().starts_with(',') {
            return true;
        }
        rest = after;
    }
    false
}

/// Whether `key:` is in the code.
fn has_key(text: &str, key: &str) -> bool {
    code(text).contains(&format!("{key}:"))
}

/// The requirement `elixir: "..."` gives, as written.
fn requirement(text: &str) -> Option<String> {
    let code = code(text);
    let at = code.find("elixir:")?;
    let rest = code[at + "elixir:".len()..].trim_start().strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_string())
}

/// The version a requirement starts from: `~> 1.15` and `>= 1.14.0` are
/// 1.15 and 1.14.0. `==` is that exact one; a requirement with no number
/// asks for nothing.
fn minimum(requirement: &str) -> Option<String> {
    let first = requirement.split(" or ").next()?.split(" and ").next()?.trim();
    let number = first.trim_start_matches(|c: char| !c.is_ascii_digit());
    let end = number.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(number.len());
    let version = number[..end].trim_end_matches('.');
    (!version.is_empty()).then(|| version.to_string())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh folder for a test.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-elixir-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn mix_exs_names_what_the_project_depends_on_and_needs() {
        let dir = temp("read");
        std::fs::write(dir.join("mix.exs"), include_str!("samples/mix.exs.phoenix")).unwrap();
        std::fs::write(dir.join(".formatter.exs"), "[\n  plugins: [Phoenix.LiveView.HTMLFormatter],\n  inputs: [\"*.{heex,ex,exs}\"]\n]\n").unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert_eq!(
            p,
            Project { mix: true, umbrella: false, phoenix: true, live_view: true, credo: true, heex_formatter: true, elixir: Some("1.15".into()) }
        );

        // A library with a dependency that's only named like Phoenix, and a
        // commented-out one.
        std::fs::write(dir.join("mix.exs"), "defmodule X.MixProject do\n  defp deps do\n    [\n      {:phoenix_html, \"~> 4.0\"},\n      # {:credo, \"~> 1.7\"},\n      {:jason, \">= 1.0.0\"}\n    ]\n  end\n  def project, do: [app: :x, elixir: \">= 1.14.0 or ~> 2.0\", deps: deps()]\nend\n").unwrap();
        std::fs::remove_file(dir.join(".formatter.exs")).unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert_eq!((p.mix, p.phoenix, p.live_view, p.credo, p.heex_formatter), (true, false, false, false, false));
        assert_eq!(p.elixir.as_deref(), Some("1.14.0"));

        // An umbrella; Credo through its own config file.
        std::fs::write(dir.join("mix.exs"), "defmodule U.MixProject do\n  def project do\n    [\n      apps_path: \"apps\",\n      deps: []\n    ]\n  end\nend\n").unwrap();
        std::fs::write(dir.join(".credo.exs"), "%{}\n").unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert!(p.umbrella && p.credo && p.elixir.is_none());

        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!Project::read(&dir.to_string_lossy()).is_elixir());
    }

    #[test]
    fn a_requirement_gives_its_lowest_version() {
        for (req, want) in [("~> 1.15", Some("1.15")), (">= 1.14.0", Some("1.14.0")), ("== 1.16.2", Some("1.16.2")), ("~> 1.15 or ~> 1.16", Some("1.15")), (">= 1.13.0 and < 2.0.0", Some("1.13.0")), ("!= x", None)] {
            assert_eq!(minimum(req).as_deref(), want, "{req}");
        }
    }
}
