//! CMake's file API: a query left in each build folder
//! (`.cmake/api/v1/query/client-fenix/codemodel-v2`) makes the next
//! configure write its code model, which says each target's program by
//! its full path. That's what the debugger and the run tasks start.

use std::path::Path;

use fenix_lang_kit::serde_json::{self, Value};

/// The query, under a build folder.
pub const QUERY: &str = ".cmake/api/v1/query/client-fenix/codemodel-v2";
const REPLY: &str = ".cmake/api/v1/reply";

/// A program the build makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Executable {
    /// The target's name (`orbit_app`).
    pub name: String,
    /// Its program, as a full path on this machine.
    pub program: String,
    /// The project's folder, as a full path on this machine.
    pub source: String,
}

/// The query left in `build` (under `root`), so CMake answers it next
/// time it configures; made with the folder, when there's none yet.
pub fn ask(root: &Path, build: &str) -> Result<(), String> {
    let file = root.join(build).join(QUERY);
    if file.is_file() {
        return Ok(());
    }
    let dir = file.parent().ok_or("no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::write(&file, "").map_err(|e| format!("{}: {e}", file.display()))
}

fn json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The newest reply's index in `build`, by its name.
pub fn index(root: &Path, build: &str) -> Option<String> {
    let dir = root.join(build).join(REPLY);
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("index-") && n.ends_with(".json")).max()
}

/// The programs CMake's last answer in `build` says the build makes:
/// none when it hasn't answered (it hasn't configured since the query).
/// A multi-config build's are its Debug ones, else its first's.
pub fn executables(root: &Path, build: &str) -> Result<Vec<Executable>, String> {
    let Some(index) = index(root, build) else { return Ok(Vec::new()) };
    let reply = root.join(build).join(REPLY);
    let idx = json(&reply.join(index))?;
    let file = idx
        .pointer("/reply/client-fenix/codemodel-v2/jsonFile")
        .and_then(Value::as_str)
        .or_else(|| idx.get("objects").and_then(Value::as_array)?.iter().find(|o| o.get("kind").and_then(Value::as_str) == Some("codemodel"))?.get("jsonFile")?.as_str())
        .ok_or("the reply has no code model")?;
    let model = json(&reply.join(file))?;
    let built = model.pointer("/paths/build").and_then(Value::as_str).unwrap_or_default().to_string();
    let source = model.pointer("/paths/source").and_then(Value::as_str).unwrap_or_default().to_string();
    let configs = model.get("configurations").and_then(Value::as_array).cloned().unwrap_or_default();
    let config = configs.iter().find(|c| c.get("name").and_then(Value::as_str) == Some("Debug")).or(configs.first());
    let mut out = Vec::new();
    for t in config.and_then(|c| c.get("targets")).and_then(Value::as_array).into_iter().flatten() {
        let Some(file) = t.get("jsonFile").and_then(Value::as_str) else { continue };
        let target = json(&reply.join(file))?;
        if target.get("type").and_then(Value::as_str) != Some("EXECUTABLE") {
            continue;
        }
        let Some(path) = target.pointer("/artifacts/0/path").and_then(Value::as_str) else { continue };
        let program = if path.starts_with('/') || path.get(1..2) == Some(":") { path.to_string() } else { format!("{}/{path}", built.trim_end_matches('/')) };
        let name = target.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
        out.push(Executable { name, program, source: source.clone() });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::project::tests::temp;

    /// The reply CMake 3.28 wrote for the sample project (a library, a
    /// program, its tests), copied into `build` under `dir`.
    pub fn reply(dir: &Path, build: &str) {
        let reply = dir.join(build).join(REPLY);
        std::fs::create_dir_all(&reply).unwrap();
        for (name, text) in [
            ("index-2026-10-29T05-48-24-0467.json", include_str!("samples/fileapi/index-2026-10-29T05-48-24-0467.json")),
            ("codemodel-v2-0af1f514d8a1cb36ba41.json", include_str!("samples/fileapi/codemodel-v2-0af1f514d8a1cb36ba41.json")),
            ("target-orbit-Debug-d2571a753ada182db815.json", include_str!("samples/fileapi/target-orbit-Debug-d2571a753ada182db815.json")),
            ("target-orbit_app-Debug-07e689aa02b980f9e9c6.json", include_str!("samples/fileapi/target-orbit_app-Debug-07e689aa02b980f9e9c6.json")),
            ("target-orbit_tests-Debug-03f110a87ef055c0c28e.json", include_str!("samples/fileapi/target-orbit_tests-Debug-03f110a87ef055c0c28e.json")),
        ] {
            std::fs::write(reply.join(name), text).unwrap();
        }
    }

    #[test]
    fn the_query_is_left_once_and_the_reply_read() {
        let dir = temp("fileapi");
        assert_eq!(executables(&dir, "build/debug").unwrap(), [], "no reply yet");
        ask(&dir, "build/debug").unwrap();
        assert!(dir.join("build/debug").join(QUERY).is_file());
        ask(&dir, "build/debug").unwrap();
        reply(&dir, "build/debug");
        let found = executables(&dir, "build/debug").unwrap();
        let rows: Vec<(&str, &str)> = found.iter().map(|e| (e.name.as_str(), e.program.as_str())).collect();
        assert_eq!(
            rows,
            [("orbit_app", "/home/user/work/cpp-pack/sample/build/debug/bin/orbit"), ("orbit_tests", "/home/user/work/cpp-pack/sample/build/debug/orbit_tests")],
            "the library isn't a program; OUTPUT_NAME and RUNTIME_OUTPUT_DIRECTORY are kept"
        );
        assert_eq!(found[0].source, "/home/user/work/cpp-pack/sample");
        assert!(index(&dir, "build/debug").unwrap().starts_with("index-"));
        std::fs::write(dir.join("build/debug/.cmake/api/v1/reply/index-2099-01-01T00-00-00-0000.json"), "{").unwrap();
        assert!(executables(&dir, "build/debug").is_err(), "the newest is read");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
