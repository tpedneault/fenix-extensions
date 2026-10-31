//! The formatter: the project's own Pint, else its PHP CS Fixer, else
//! the language server's formatting. Fenix runs a formatter on the
//! buffer's text, on its stdin, and takes its stdout; Pint and PHP CS
//! Fixer only fix files (PHP CS Fixer reads stdin, but only to say what
//! it would change), so PHP runs them on a copy of the text and gives it
//! back -- the project's config read from where they run, the project.

use fenix_extension_api::Command;

use crate::project::Project;
use crate::tools::Tools;

/// What formats PHP (`php.formatter`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Formatter {
    /// Pint or PHP CS Fixer, as the project has; else the server's.
    #[default]
    Project,
    /// The language server's.
    Server,
}

impl Formatter {
    pub fn parse(text: &str) -> Formatter {
        if text.trim().eq_ignore_ascii_case("server") {
            Formatter::Server
        } else {
            Formatter::Project
        }
    }
}

/// `php -r`'s code: the text written to a `.php` file of its own, the
/// tool (the args after `--`) run on it, and the file written back; the
/// tool's output, and its exit code, when it fails.
pub const ON_A_COPY: &str = r#"$t = tempnam(sys_get_temp_dir(), "fenix"); $f = "$t.php"; rename($t, $f); file_put_contents($f, stream_get_contents(STDIN)); exec(implode(" ", array_map("escapeshellarg", array_merge(array_slice($argv, 1), [$f]))) . " 2>&1", $said, $code); $out = file_get_contents($f); unlink($f); if ($code !== 0) { fwrite(STDERR, implode("\n", $said) . "\n"); exit($code); } echo $out;"#;

/// `tool` and its args, run by PHP on a copy of the buffer.
fn on_a_copy(tools: &Tools, tool: &str, args: &[&str]) -> Command {
    tools.php().args(["-r", ON_A_COPY, "--", tool]).args(args.iter().copied())
}

/// The formatter for `project`, with what's found: Pint where the
/// project uses it, else PHP CS Fixer (with its config); none leaves it
/// to the server.
pub fn formatter(project: &Project, tools: &Tools, wanted: Formatter) -> Option<Command> {
    if wanted == Formatter::Server || tools.php.is_none() {
        return None;
    }
    if let (true, Some(pint)) = (project.uses_pint(), &tools.pint) {
        return Some(on_a_copy(tools, pint, &["--quiet"]));
    }
    if let (true, Some(fixer)) = (project.uses_cs_fixer(), &tools.php_cs_fixer) {
        let config = project.cs_fixer_config.as_ref().map(|c| format!("--config={c}"));
        let mut args = vec!["fix", "--quiet", "--no-interaction", "--using-cache=no"];
        args.extend(config.as_deref());
        return Some(on_a_copy(tools, fixer, &args));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    fn tools() -> Tools {
        Tools { php: Some("/usr/bin/php".into()), pint: Some("/p/vendor/bin/pint".into()), php_cs_fixer: Some("/p/vendor/bin/php-cs-fixer".into()), ..Tools::default() }
    }

    #[test]
    fn pint_then_php_cs_fixer_as_the_project_has_them() {
        let pint = Project { pint_config: true, ..Project::default() };
        let cmd = formatter(&pint, &tools(), Formatter::Project).unwrap();
        assert_eq!(cmd.program, "/usr/bin/php");
        assert_eq!(cmd.args[0..2], ["-r".to_string(), ON_A_COPY.to_string()]);
        assert_eq!(cmd.args[2..], ["--", "/p/vendor/bin/pint", "--quiet"]);
        let fixer = Project { cs_fixer_config: Some(".php-cs-fixer.dist.php".into()), ..Project::default() };
        let cmd = formatter(&fixer, &tools(), Formatter::Project).unwrap();
        assert!(line(&cmd).ends_with("-- /p/vendor/bin/php-cs-fixer fix --quiet --no-interaction --using-cache=no --config=.php-cs-fixer.dist.php"), "{}", line(&cmd));
        assert_eq!(formatter(&Project::default(), &tools(), Formatter::Project), None, "neither: the server's");
        assert_eq!(formatter(&pint, &tools(), Formatter::Server), None);
        assert_eq!(formatter(&pint, &Tools { php: None, ..tools() }, Formatter::Project), None, "no PHP to run it");
        assert_eq!((Formatter::parse("Server"), Formatter::parse("")), (Formatter::Server, Formatter::Project));
    }

    /// The code, run by this machine's PHP when it has one, on a tool of
    /// the test's own that capitalises a file, or fails.
    #[test]
    fn php_formats_a_copy_and_gives_it_back() {
        use std::io::Write;
        use std::process::{Command as Process, Stdio};
        if Process::new("php").arg("--version").output().is_err() {
            return;
        }
        let dir = crate::project::tests::temp("format");
        let tool = dir.join("upper.php");
        std::fs::write(&tool, "<?php $f = end($argv); if (str_contains(file_get_contents($f), 'BROKEN')) { echo \"syntax error\\n\"; exit(4); } file_put_contents($f, strtoupper(file_get_contents($f)));").unwrap();
        let run = |text: &str| {
            let mut child = Process::new("php").args(["-r", ON_A_COPY, "--", "php"]).arg(&tool).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
            child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
            child.wait_with_output().unwrap()
        };
        let out = run("<?php echo 'orbit';\n");
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout), "<?PHP ECHO 'ORBIT';\n");
        let out = run("BROKEN");
        assert_eq!(out.status.code(), Some(4));
        assert!(out.stdout.is_empty() && String::from_utf8_lossy(&out.stderr).contains("syntax error"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
