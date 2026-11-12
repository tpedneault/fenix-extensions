//! `cargo xtask install <extension-dir> [--fenix <path>] [--watch]`: pack an
//! extension and install it into Fenix, in one step -- what
//! `package.ps1 -Install` did, on Windows and Linux alike.
//!
//! ```text
//! cargo xtask install extensions/dusk                  pack it, then: fenix --install-extension <the .fenixext>
//! cargo xtask install extensions/dusk --fenix ~/fenix  a Fenix that isn't on the PATH ($FENIX does the same)
//! cargo xtask install extensions/dusk --watch          and again whenever a file under it changes; Ctrl+C stops
//! ```
//!
//! The package is made in a folder of its own under the system's temp
//! folder, not in `dist/`, and removed when this ends; `pack` is the task
//! that leaves a package behind. A running Fenix notices what the command
//! line installs within a few seconds (a theme or a language wait for
//! `SPC q r`).
//!
//! `--watch` polls: every half second it reads the modification time and
//! size of each file under the extension -- leaving out `target/`,
//! `dist/`, `.git/`, the `extension.wasm` a build makes, and editors'
//! scratch files -- and when something differs it waits until the folder
//! has held still for a moment (a save is often several writes) before it
//! packs and installs again. No dependency, no platform's file-watching
//! API; the price is the half second, and reading every file's metadata
//! that often, which for an extension's folder is nothing.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::screenshots::default_fenix;

const USAGE: &str = "cargo xtask install <extension-dir> [--fenix <path>] [--watch]";

/// How often `--watch` looks, and how long the folder must hold still
/// before it packs.
const POLL: Duration = Duration::from_millis(500);
const DEBOUNCE: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub dir: PathBuf,
    pub fenix: PathBuf,
    pub watch: bool,
}

/// The command line, with `fenix` where `--fenix` doesn't say.
pub fn options(args: &[String], fenix: PathBuf) -> Result<Options, String> {
    let (mut dir, mut fenix, mut watch) = (None, fenix, false);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        match flag {
            "--fenix" => fenix = PathBuf::from(inline.or_else(|| args.next().cloned()).filter(|f| !f.is_empty()).ok_or("--fenix needs the path of Fenix")?),
            "--watch" if inline.is_none() => watch = true,
            f if f.starts_with("--") => return Err(format!("install: {f}? -- {USAGE}")),
            _ if dir.is_none() => dir = Some(PathBuf::from(arg)),
            _ => return Err(format!("install: one extension at a time ({arg}?) -- {USAGE}")),
        }
    }
    Ok(Options { dir: dir.ok_or_else(|| USAGE.to_string())?, fenix, watch })
}

/// Why an install didn't happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// There's no Fenix to run: trying again won't help.
    Missing(String),
    /// The build, the package or Fenix said no: fix it and try again.
    Other(String),
}

impl Failure {
    pub fn message(&self) -> &str {
        match self {
            Failure::Missing(m) | Failure::Other(m) => m,
        }
    }
}

/// What runs a program: its arguments, and how it ended (its exit code;
/// none when a signal stopped it). Fenix's, or a test's.
pub type Runner<'a> = dyn FnMut(&Path, &[OsString]) -> std::io::Result<Option<i32>> + 'a;

/// The arguments `fenix` is run with to install `package`: Fenix's
/// `--install-extension`, which takes a `.fenixext`, a folder or an id.
pub fn install_args(package: &Path) -> Vec<OsString> {
    vec!["--install-extension".into(), package.into()]
}

fn not_found(fenix: &Path) -> String {
    format!("Fenix wasn't found: there's no program `{}`. Put fenix on the PATH, set $FENIX to it, or say where it is with --fenix <path>", fenix.display())
}

/// Installs `package` with `fenix`, through `run`.
pub fn install_package(fenix: &Path, package: &Path, run: &mut Runner) -> Result<(), Failure> {
    let args = install_args(package);
    match run(fenix, &args) {
        Ok(Some(0)) => Ok(()),
        Ok(Some(code)) => Err(Failure::Other(format!("{} --install-extension {} failed (exit code {code})", fenix.display(), package.display()))),
        Ok(None) => Err(Failure::Other(format!("{} --install-extension {} was stopped before it finished", fenix.display(), package.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(Failure::Missing(not_found(fenix))),
        Err(e) => Err(Failure::Missing(format!("{}: {e} -- say where Fenix is with --fenix <path>", fenix.display()))),
    }
}

/// Where `program` is, as a shell would find it: a path as it is (the
/// extension tried after it, if it has none), a bare name in each folder of
/// `path`, in order. `exts` are what a name may be given on Windows
/// (`PATHEXT`: `.exe`, `.cmd`); none elsewhere.
pub fn which(program: &Path, path: Option<&std::ffi::OsStr>, exts: &[String]) -> Option<PathBuf> {
    let found = |candidate: PathBuf| -> Option<PathBuf> {
        if candidate.is_file() {
            return Some(candidate);
        }
        exts.iter().map(|e| PathBuf::from(format!("{}{e}", candidate.display()))).find(|c| c.is_file())
    };
    if program.components().count() > 1 || program.is_absolute() {
        return found(program.to_path_buf());
    }
    std::env::split_paths(path?).find_map(|dir| found(dir.join(program)))
}

/// What the system says a program's name may end in: `PATHEXT` on Windows.
fn path_exts() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into()).split(';').filter(|e| !e.is_empty()).map(str::to_string).collect()
}

/// Runs the real program, with this process's input and output.
fn system(program: &Path, args: &[OsString]) -> std::io::Result<Option<i32>> {
    std::process::Command::new(program).args(args).status().map(|s| s.code())
}

/// A folder for the package, gone when this is dropped.
struct Dist(PathBuf);

impl Dist {
    fn new() -> Dist {
        Dist(std::env::temp_dir().join(format!("xtask-install-{}", std::process::id())))
    }
}

impl Drop for Dist {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One round: the extension packed into `dist` (built first, if it has
/// code), the package installed. Its path.
pub fn cycle(dir: &Path, dist: &Path, fenix: &Path, run: &mut Runner) -> Result<PathBuf, Failure> {
    let m = crate::manifest(dir).map_err(Failure::Other)?;
    let package = dist.join(format!("{}-{}.fenixext", m.extension.id, m.extension.version));
    let _ = std::fs::remove_file(&package);
    crate::pack(dir, dist).map_err(Failure::Other)?;
    if !package.is_file() {
        return Err(Failure::Other(format!("{} wasn't made", package.display())));
    }
    install_package(fenix, &package, run)?;
    Ok(package)
}

// ------------------------------------------------------------------
// --watch
// ------------------------------------------------------------------

/// Each file under the extension, by its path there: when it was last
/// written, and how long it is.
pub type Snapshot = BTreeMap<PathBuf, (SystemTime, u64)>;

/// What a change to doesn't mean the extension changed: what a build
/// makes, and Git's and editors' own files.
fn ignored(rel: &Path) -> bool {
    let first = rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
    let name = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    matches!(first.as_str(), "target" | "dist" | ".git") || rel == Path::new("extension.wasm") || name.ends_with('~') || name.ends_with(".swp") || name.ends_with(".swx") || name.starts_with(".#")
}

/// Reads `dir`: every file that isn't ignored. A file that can't be read
/// (it went between the listing and the look) isn't there.
pub fn snapshot(dir: &Path) -> Snapshot {
    let mut out = Snapshot::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        for entry in std::fs::read_dir(dir.join(&rel)).into_iter().flatten().flatten() {
            let path = rel.join(entry.file_name());
            if ignored(&path) {
                continue;
            }
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                stack.push(path);
            } else if let Ok(meta) = entry.metadata() {
                out.insert(path, (meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len()));
            }
        }
    }
    out
}

/// The files that differ between two snapshots: written, added or
/// removed, sorted.
pub fn changes(before: &Snapshot, after: &Snapshot) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = after.iter().filter(|(p, v)| before.get(*p) != Some(v)).map(|(p, _)| p.clone()).chain(before.keys().filter(|p| !after.contains_key(*p)).cloned()).collect();
    out.sort();
    out
}

/// Waits (`sleep`, once each `poll`) until something under `dir` differs
/// from `seen`, then until the folder has held still for `debounce`:
/// the snapshot it settled on, and what changed from `seen`.
pub fn next_change(dir: &Path, seen: &Snapshot, poll: Duration, debounce: Duration, sleep: &mut dyn FnMut(Duration)) -> (Snapshot, Vec<PathBuf>) {
    loop {
        sleep(poll);
        let mut now = snapshot(dir);
        if now == *seen {
            continue;
        }
        loop {
            sleep(debounce);
            let again = snapshot(dir);
            if again == now {
                break;
            }
            now = again;
        }
        let changed = changes(seen, &now);
        // Something touched and put back as it was: nothing to do.
        if changed.is_empty() {
            return next_change(dir, &now, poll, debounce, sleep);
        }
        return (now, changed);
    }
}

fn changed_said(changed: &[PathBuf]) -> String {
    let shown: Vec<String> = changed.iter().take(3).map(|p| p.display().to_string()).collect();
    match changed.len() {
        0..=3 => shown.join(", "),
        n => format!("{}, and {} more", shown.join(", "), n - 3),
    }
}

/// Installs, then installs again each time `dir` changes, until `more`
/// says stop. A round that fails is said and waited out -- the next save
/// may fix it -- unless there's no Fenix to run, which no save fixes.
/// `say` is where it talks.
pub fn watch(
    dir: &Path,
    cycle: &mut dyn FnMut() -> Result<(), Failure>,
    sleep: &mut dyn FnMut(Duration),
    say: &mut dyn FnMut(&str),
    more: &mut dyn FnMut() -> bool,
) -> Result<(), String> {
    let mut round = |say: &mut dyn FnMut(&str)| -> Result<(), String> {
        match cycle() {
            Ok(()) => say("installed"),
            Err(Failure::Missing(m)) => return Err(m),
            Err(Failure::Other(m)) => say(&format!("{m}\nnot installed; waiting for the next change")),
        }
        Ok(())
    };
    let mut seen = snapshot(dir);
    round(say)?;
    say(&format!("watching {} for changes (Ctrl+C stops)", dir.display()));
    while more() {
        // What's read now is what's packed: a save during the build shows as the next change.
        let (now, changed) = next_change(dir, &seen, POLL, DEBOUNCE, sleep);
        seen = now;
        say(&format!("changed: {}", changed_said(&changed)));
        round(say)?;
    }
    Ok(())
}

pub fn run(args: &[String]) -> Result<(), String> {
    let o = options(args, default_fenix())?;
    // Before the build, which can take a minute: is there a Fenix to hand it to?
    if which(&o.fenix, std::env::var_os("PATH").as_deref(), &path_exts()).is_none() {
        return Err(not_found(&o.fenix));
    }
    let dist = Dist::new();
    let mut run = system;
    let mut once = || cycle(&o.dir, &dist.0, &o.fenix, &mut run).map(|_| ());
    if !o.watch {
        once().map_err(|f| f.message().to_string())?;
        eprintln!("installed {} into Fenix", o.dir.display());
        return Ok(());
    }
    watch(&o.dir, &mut once, &mut std::thread::sleep, &mut |s| eprintln!("{s}"), &mut || true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Temp {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("xtask-install-test-{name}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("extensions")).unwrap();
            Temp(dir)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn the_command_line_is_read() {
        let default = PathBuf::from("fenix");
        let o = options(&args("extensions/dusk"), default.clone()).unwrap();
        assert_eq!(o, Options { dir: "extensions/dusk".into(), fenix: default.clone(), watch: false });
        let o = options(&args("--watch extensions/dusk --fenix /opt/fenix/fenix"), default.clone()).unwrap();
        assert_eq!((o.watch, o.fenix), (true, PathBuf::from("/opt/fenix/fenix")));
        assert_eq!(options(&args("extensions/dusk --fenix=C:\\Fenix\\fenix.exe"), default.clone()).unwrap().fenix, PathBuf::from("C:\\Fenix\\fenix.exe"));
        assert!(options(&[], default.clone()).unwrap_err().contains("<extension-dir>"));
        assert!(options(&args("a b"), default.clone()).unwrap_err().contains("one extension at a time"));
        assert!(options(&args("a --fenix"), default.clone()).unwrap_err().contains("--fenix needs"));
        assert!(options(&args("a --follow"), default.clone()).unwrap_err().contains("--follow"));
        assert!(options(&args("a --watch=yes"), default).is_err());
    }

    #[test]
    fn fenix_is_run_with_its_install_flag_and_the_package() {
        let package = Path::new("/tmp/dist/acme.dusk-1.0.0.fenixext");
        assert_eq!(install_args(package), [OsString::from("--install-extension"), package.into()]);
        let calls = RefCell::new(Vec::new());
        let mut run = |program: &Path, args: &[OsString]| {
            calls.borrow_mut().push((program.to_path_buf(), args.to_vec()));
            Ok(Some(0))
        };
        install_package(Path::new("/opt/fenix"), package, &mut run).unwrap();
        assert_eq!(*calls.borrow(), [(PathBuf::from("/opt/fenix"), install_args(package))]);
    }

    #[test]
    fn a_fenix_that_isnt_there_is_said_with_the_flag_that_names_it() {
        let mut run = |_: &Path, _: &[OsString]| Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        let err = install_package(Path::new("fenix"), Path::new("x.fenixext"), &mut run).unwrap_err();
        let Failure::Missing(said) = err else { panic!("{err:?}") };
        assert!(said.contains("Fenix wasn't found") && said.contains("`fenix`") && said.contains("--fenix <path>") && said.contains("$FENIX") && said.contains("PATH"), "{said}");
        let mut run = |_: &Path, _: &[OsString]| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        let said = install_package(Path::new("/x/fenix"), Path::new("x.fenixext"), &mut run).unwrap_err();
        assert!(matches!(&said, Failure::Missing(m) if m.contains("/x/fenix") && m.contains("--fenix")), "{said:?}");
    }

    #[test]
    fn fenix_is_looked_for_as_a_shell_would() {
        let t = Temp::new("which");
        let (a, b) = (t.0.join("a"), t.0.join("b"));
        write(&a.join("other"), "");
        write(&b.join("fenix"), "");
        write(&b.join("fenix2.exe"), "");
        let path = std::env::join_paths([&a, &b]).unwrap();
        let find = |program: &str, exts: &[&str]| which(Path::new(program), Some(path.as_os_str()), &exts.iter().map(|e| e.to_string()).collect::<Vec<_>>());
        assert_eq!(find("fenix", &[]), Some(b.join("fenix")), "the first folder that has it");
        assert_eq!(find("fenix2", &[".cmd", ".exe"]), Some(b.join("fenix2.exe")), "Windows: PATHEXT's endings");
        assert_eq!(find("fenix2", &[]), None);
        assert_eq!(find("nothing", &[]), None);
        assert_eq!(which(&b.join("fenix"), None, &[]), Some(b.join("fenix")), "a path needs no PATH");
        assert_eq!(which(&b.join("fenix2"), None, &[".exe".into()]), Some(b.join("fenix2.exe")));
        assert_eq!(which(&b.join("missing"), Some(path.as_os_str()), &[]), None, "a path isn't looked for in PATH");
        assert_eq!(which(Path::new("fenix"), None, &[]), None, "no PATH, nothing to look in");
    }

    #[test]
    fn a_fenix_that_says_no_is_not_a_missing_one() {
        let mut run = |_: &Path, _: &[OsString]| Ok(Some(2));
        let err = install_package(Path::new("fenix"), Path::new("x.fenixext"), &mut run).unwrap_err();
        assert!(matches!(&err, Failure::Other(m) if m.contains("exit code 2") && m.contains("x.fenixext")), "{err:?}");
        let mut run = |_: &Path, _: &[OsString]| Ok(None);
        assert!(matches!(install_package(Path::new("fenix"), Path::new("x.fenixext"), &mut run), Err(Failure::Other(m)) if m.contains("stopped")));
    }

    #[test]
    fn an_extension_is_packed_and_its_package_handed_to_fenix() {
        let t = Temp::new("cycle");
        let w = crate::new::scaffold(&t.0, &crate::new::Options { kind: "theme".into(), id: "acme.dusk".into(), name: None, fenix: false }).unwrap();
        let dist = t.0.join("dist");
        let seen = RefCell::new(Vec::new());
        let mut run = |program: &Path, args: &[OsString]| {
            // The package is there when Fenix is asked to install it.
            let package = PathBuf::from(&args[1]);
            seen.borrow_mut().push((program.to_path_buf(), args[0].clone(), package.file_name().map(|n| n.to_os_string()), package.is_file()));
            Ok(Some(0))
        };
        let package = cycle(&w.dir, &dist, Path::new("/opt/fenix"), &mut run).unwrap();
        assert_eq!(package, dist.join("acme.dusk-1.0.0.fenixext"));
        assert_eq!(*seen.borrow(), [(PathBuf::from("/opt/fenix"), OsString::from("--install-extension"), Some(OsString::from("acme.dusk-1.0.0.fenixext")), true)]);

        // No Fenix: the package was made, and the error says so.
        let mut missing = |_: &Path, _: &[OsString]| Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert!(matches!(cycle(&w.dir, &dist, Path::new("fenix"), &mut missing), Err(Failure::Missing(_))));
        // Not an extension: the manifest says what's wrong, and Fenix isn't run.
        let called = Cell::new(false);
        let mut never = |_: &Path, _: &[OsString]| {
            called.set(true);
            Ok(Some(0))
        };
        let err = cycle(&t.0.join("extensions/nothing"), &dist, Path::new("fenix"), &mut never).unwrap_err();
        assert!(matches!(&err, Failure::Other(m) if m.contains("extension.toml")), "{err:?}");
        assert!(!called.get());
    }

    #[test]
    fn a_snapshot_leaves_out_what_a_build_makes() {
        let t = Temp::new("snapshot");
        let d = &t.0;
        write(&d.join("extension.toml"), "x");
        write(&d.join("src/lib.rs"), "fn main() {}");
        write(&d.join("themes/a.toml"), "name = \"a\"");
        write(&d.join("grammars/g.wasm"), "wasm");
        for ignored in ["extension.wasm", "target/release/x.wasm", "dist/a.fenixext", ".git/HEAD", "src/lib.rs~", "src/.lib.rs.swp", "src/.#lib.rs"] {
            write(&d.join(ignored), "build");
        }
        let names: Vec<String> = snapshot(d).keys().map(|p| p.to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(names, ["extension.toml", "grammars/g.wasm", "src/lib.rs", "themes/a.toml"], "a wasm that's the extension's own data stays");
    }

    #[test]
    fn changes_are_written_added_and_removed_files() {
        let t = Temp::new("changes");
        let d = &t.0;
        write(&d.join("extension.toml"), "one");
        write(&d.join("a.txt"), "a");
        write(&d.join("b.txt"), "b");
        let before = snapshot(d);
        assert!(changes(&before, &snapshot(d)).is_empty());
        write(&d.join("a.txt"), "a, longer");
        write(&d.join("c/new.txt"), "c");
        std::fs::remove_file(d.join("b.txt")).unwrap();
        write(&d.join("target/x"), "ignored");
        write(&d.join("extension.wasm"), "ignored");
        let after = snapshot(d);
        let said: Vec<String> = changes(&before, &after).iter().map(|p| p.to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(said, ["a.txt", "b.txt", "c/new.txt"]);
        assert_eq!(changed_said(&changes(&before, &after)), "a.txt, b.txt, c/new.txt");
        assert_eq!(changed_said(&["1", "2", "3", "4", "5"].map(PathBuf::from)), "1, 2, 3, and 2 more");
    }

    #[test]
    fn a_change_waits_for_the_folder_to_hold_still() {
        let t = Temp::new("debounce");
        let d = t.0.clone();
        write(&d.join("extension.toml"), "one");
        let seen = snapshot(&d);
        // What happens at each sleep: nothing; a save; the save again
        // (still being written); then quiet.
        let sleeps = Cell::new(0);
        let log = RefCell::new(Vec::new());
        let mut sleep = |for_: Duration| {
            sleeps.set(sleeps.get() + 1);
            log.borrow_mut().push(for_);
            match sleeps.get() {
                2 => write(&d.join("extension.toml"), "one, edited"),
                3 => write(&d.join("extension.toml"), "one, edited again"),
                _ => {}
            }
        };
        let (now, changed) = next_change(&t.0, &seen, POLL, DEBOUNCE, &mut sleep);
        assert_eq!(changed, [PathBuf::from("extension.toml")]);
        assert_eq!(now, snapshot(&t.0), "what it settled on is what's there");
        // poll (nothing), poll (the save), debounce (it changed again), debounce (still).
        assert_eq!(*log.borrow(), [POLL, POLL, DEBOUNCE, DEBOUNCE]);
    }

    #[test]
    fn something_put_back_as_it_was_is_not_a_change() {
        let t = Temp::new("putback");
        let d = t.0.clone();
        write(&d.join("a.txt"), "same");
        write(&d.join("b.txt"), "x");
        let seen = snapshot(&d);
        let meta = std::fs::metadata(d.join("a.txt")).unwrap();
        let sleeps = Cell::new(0);
        let mut sleep = |_: Duration| {
            sleeps.set(sleeps.get() + 1);
            match sleeps.get() {
                // Touched, then put back: the same time and length.
                1 => write(&d.join("a.txt"), "other"),
                2 => {
                    write(&d.join("a.txt"), "same");
                    std::fs::File::options().write(true).open(d.join("a.txt")).unwrap().set_modified(meta.modified().unwrap()).unwrap();
                }
                // Then a real one.
                4 => write(&d.join("b.txt"), "xy"),
                _ => {}
            }
        };
        let (_, changed) = next_change(&t.0, &seen, POLL, DEBOUNCE, &mut sleep);
        assert_eq!(changed, [PathBuf::from("b.txt")]);
    }

    #[test]
    fn watching_installs_once_then_on_each_change_and_says_when_a_round_fails() {
        let t = Temp::new("watch");
        let d = t.0.clone();
        write(&d.join("extension.toml"), "1");
        let rounds = Cell::new(0);
        let mut cycle = || {
            rounds.set(rounds.get() + 1);
            if rounds.get() == 2 { Err(Failure::Other("the build failed".into())) } else { Ok(()) }
        };
        // Each poll sees a new save, until the loop is told to stop.
        let polls = Cell::new(0);
        let mut sleep = |for_: Duration| {
            if for_ == POLL {
                polls.set(polls.get() + 1);
                write(&d.join("extension.toml"), &"x".repeat(polls.get() + 1));
            }
        };
        let said = RefCell::new(Vec::new());
        let mut say = |s: &str| said.borrow_mut().push(s.to_string());
        let asked = Cell::new(0);
        let mut more = || {
            asked.set(asked.get() + 1);
            asked.get() <= 2
        };
        watch(&t.0, &mut cycle, &mut sleep, &mut say, &mut more).unwrap();
        assert_eq!(rounds.get(), 3, "once to start, once for each of two changes");
        let said = said.borrow();
        assert_eq!(said[0], "installed");
        assert!(said[1].starts_with("watching "), "{said:?}");
        assert!(said[2].contains("changed: extension.toml") && said[3].contains("the build failed") && said[3].contains("waiting for the next change"), "{said:?}");
        assert_eq!(said[5], "installed", "the next save installed it");
    }

    #[test]
    fn watching_stops_when_there_is_no_fenix() {
        let t = Temp::new("watch-missing");
        write(&t.0.join("extension.toml"), "1");
        let mut cycle = || Err(Failure::Missing("Fenix wasn't found".into()));
        let mut more = || panic!("it should have stopped");
        let err = watch(&t.0, &mut cycle, &mut |_| {}, &mut |_| {}, &mut more).unwrap_err();
        assert_eq!(err, "Fenix wasn't found");
    }
}
