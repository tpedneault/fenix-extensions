//! Tasks with a profile's variables (API 0.8.3): the commands
//! `dotenv.tasks` lists, offered in the task picker, each run in the
//! project with the variables of the profile picked for them -- what
//! `.env.staging` holds, without copying it over `.env`.

/// The programs a task may run: those the manifest's
/// `permissions.process` lists beside Git. Fenix runs no other.
pub const RUNNERS: &[&str] = &[
    "npm", "npx", "pnpm", "yarn", "bun", "node", "deno", "python", "python3", "uv", "poetry", "pipenv", "cargo", "go", "make", "just", "docker", "dotnet", "php", "ruby", "bundle", "rails",
];

/// A command line's words: split at spaces, a quoted run ("…" or '…')
/// one word, its quotes taken off.
pub fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut any = false;
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if any || !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
                any = false;
            }
            (None, c) => word.push(c),
        }
    }
    if any || !word.is_empty() {
        out.push(word);
    }
    out
}

/// A task's program and its arguments, from its command line: the
/// program one of [`RUNNERS`].
pub fn command(line: &str) -> Result<(String, Vec<String>), String> {
    let mut words = words(line);
    if words.is_empty() {
        return Err("no command".into());
    }
    let program = words.remove(0);
    let stem = program.strip_suffix(".exe").unwrap_or(&program);
    if !RUNNERS.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return Err(format!("{program}: not a program it may run -- one of {}", RUNNERS.join(", ")));
    }
    Ok((program, words))
}

/// The task's name in the picker: the command, and the profile it runs
/// with.
pub fn name(line: &str, profile: &str) -> String {
    format!("{} · env {profile}", line.trim())
}

/// The variables profile `text` gives a task, as they're loaded: the
/// last of a key set twice, `${VAR}` filled in.
pub fn env(text: &str) -> Vec<(String, String)> {
    let file = crate::parse::EnvFile::parse(text);
    let mut out: Vec<(String, String)> = Vec::new();
    for (e, (value, _)) in file.entries.iter().zip(file.values()) {
        out.retain(|(k, _)| *k != e.key);
        out.push((e.key.clone(), value));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_lines_are_split_as_a_shell_would() {
        assert_eq!(words("npm run dev"), ["npm", "run", "dev"]);
        assert_eq!(words("  cargo  run -- \"a b\" 'c d' \"\" "), ["cargo", "run", "--", "a b", "c d", ""]);
        assert_eq!(words("x=\"1 2\""), ["x=1 2"]);
        assert!(words("   ").is_empty());
    }

    #[test]
    fn only_the_programs_it_may_run() {
        assert_eq!(command("npm run dev").unwrap(), ("npm".to_string(), vec!["run".to_string(), "dev".into()]));
        assert!(command("cargo.exe run").is_ok());
        assert!(command("rm -rf /").unwrap_err().contains("not a program it may run"));
        assert!(command("").is_err());
    }

    #[test]
    fn the_runners_are_the_manifests() {
        let manifest = include_str!("../extension.toml");
        let line = manifest.lines().find(|l| l.starts_with("process = ")).unwrap();
        let listed: Vec<&str> = line.trim_start_matches("process = [").trim_end_matches(']').split(',').map(|s| s.trim().trim_matches('"')).collect();
        let mut want: Vec<&str> = ["git"].into_iter().chain(RUNNERS.iter().copied()).collect();
        want.sort_unstable();
        let mut listed = listed;
        listed.sort_unstable();
        assert_eq!(listed, want);
    }

    #[test]
    fn a_profiles_variables_as_loaded() {
        let env = env("A=1\nB=${A}2\nA=3\n# C=x\nexport D='q r'\n");
        assert_eq!(env, [("B".to_string(), "12".to_string()), ("A".into(), "3".into()), ("D".into(), "q r".into())]);
        assert_eq!(name(" npm run dev ", ".env.staging"), "npm run dev · env .env.staging");
    }
}
