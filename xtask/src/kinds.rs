//! `[[contributes.project_kinds]]`: the project kinds a pack declares
//! (Fenix 1.6), held to Fenix's rules (the plan's §6.2), and the kinds
//! the store's templates name.
//!
//! The section is data, read by Fenix and never by the extension's code,
//! so it isn't tied to `engine.api`: Fenix 1.0 to 1.5.2 parse
//! `[contributes]` without `deny_unknown_fields` and ignore it, and a pack
//! keeps `api = "1.5"`. The one thing an older Fenix does refuse is a
//! template naming a kind it doesn't know, so such a template needs
//! `engine.fenix` at 1.6 or later.
//!
//! Per manifest (`check_pack`, used by `validate` and by packages built
//! elsewhere): every rule of a kind's fields. Across the store
//! (`check_store`): one pack per id, no tag twice, and what each
//! template's `kind` names.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

/// The ids Fenix has built in and a pack may add markers to; everything
/// else about them is Fenix's.
pub const BUILT_IN_IDS: &[&str] = &["python", "arduino", "rust", "tcl", "cpp", "node", "go"];

/// Ids no pack may declare: the two built-ins that aren't detected, and
/// the aliases Fenix reads as a built-in (`from_id`; `mib` is the MIB
/// branch's).
const RESERVED_IDS: &[&str] = &["other", "monorepo", "py", "uv", "ino", "sketch", "rs", "c++", "c", "cmake", "js", "ts", "javascript", "typescript", "mib"];

/// The tags Fenix's own kinds use.
const FENIX_TAGS: &[&str] = &["PY", "INO", "RS", "TCL", "C++", "JS", "GO", "MONO", "DIR"];

/// The theme roles a kind may be coloured by.
const COLORS: &[&str] = &["keyword", "string", "function", "type", "number", "constant", "attribute", "variable"];

const MAX_KINDS: usize = 8;
const MAX_ID: usize = 24;
const MAX_NAME: usize = 32;
const MAX_TAG: usize = 4;
const MAX_MARKERS: usize = 16;
const MAX_MARKER: usize = 64;
const MAX_MAIN: usize = 8;
const MAX_MAIN_PATH: usize = 128;

/// One `[[contributes.project_kinds]]`, with every field Fenix reads and
/// no other (a typo is an error, as Fenix's own contributions are).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declared {
    pub id: String,
    pub name: Option<String>,
    pub tag: Option<String>,
    pub color: Option<String>,
    #[serde(default)]
    pub markers: Vec<String>,
    #[serde(default)]
    pub main: Vec<String>,
}

impl Declared {
    pub fn is_built_in(&self) -> bool {
        BUILT_IN_IDS.contains(&self.id.as_str())
    }
}

/// What a pack says about kinds: the ones it declares, and the kind each
/// of its templates names.
#[derive(Debug, Clone)]
pub struct Pack {
    pub id: String,
    pub kinds: Vec<Declared>,
    /// (template folder, its kind).
    pub templates: Vec<(String, String)>,
    pub dependencies: BTreeSet<String>,
}

/// An id as Fenix reads one (§6.2): `a-z` first, then `a-z 0-9 - _`, at
/// most 24.
fn well_formed(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase()) && id.len() <= MAX_ID && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// An id Fenix 1.5 reads in a template: its built-ins and their aliases.
fn fenix_id(id: &str) -> bool {
    BUILT_IN_IDS.contains(&id) || RESERVED_IDS.contains(&id)
}

fn check_marker(marker: &str) -> Result<(), String> {
    if marker.is_empty() || marker.len() > MAX_MARKER {
        return Err(format!("marker {marker:?} is {} bytes: 1 to {MAX_MARKER}", marker.len()));
    }
    if let Some(c) = marker.chars().find(|c| matches!(c, '/' | '\\' | '*' | '?' | '[' | ']')) {
        return Err(format!("marker {marker:?} has {c:?}: a marker is a file name at the project's top, no path, no wildcard"));
    }
    if marker.contains("..") {
        return Err(format!("marker {marker:?} has \"..\""));
    }
    if marker.eq_ignore_ascii_case(".fenix") {
        return Err(format!("marker {marker:?} is Fenix's own settings folder"));
    }
    let bare = marker.replacen("{folder}", "", 1);
    if bare.contains('{') || bare.contains('}') {
        return Err(format!("marker {marker:?}: {{folder}} at most once, and no other braces"));
    }
    Ok(())
}

fn check_main(path: &str) -> Result<(), String> {
    if path.is_empty() || path.len() > MAX_MAIN_PATH {
        return Err(format!("main {path:?} is {} bytes: 1 to {MAX_MAIN_PATH}", path.len()));
    }
    if path.contains('\\') || path.starts_with('/') {
        return Err(format!("main {path:?}: a path below the project, with / between its parts"));
    }
    // An empty part (`a//b`, a trailing `/`) isn't a path below the
    // project either: §6.2 doesn't say, so this is the strict reading.
    if path.split('/').any(|part| part.is_empty() || part == "..") {
        return Err(format!("main {path:?}: a path below the project, with no empty or \"..\" part"));
    }
    Ok(())
}

/// One declaration against §6.2, the error saying which kind and why.
fn check_kind(kind: &Declared) -> Result<(), String> {
    let id = kind.id.as_str();
    if !well_formed(id) {
        return Err(format!("id {id:?} -- 1 to {MAX_ID} characters, a-z first, then a-z 0-9 - _"));
    }
    if RESERVED_IDS.contains(&id) {
        return Err(format!("{id:?} is Fenix's own (an alias, or a kind no pack declares)"));
    }
    if kind.markers.len() > MAX_MARKERS {
        return Err(format!("{} markers -- at most {MAX_MARKERS}", kind.markers.len()));
    }
    for marker in &kind.markers {
        check_marker(marker)?;
    }
    if kind.main.len() > MAX_MAIN {
        return Err(format!("{} main files -- at most {MAX_MAIN}", kind.main.len()));
    }
    for path in &kind.main {
        check_main(path)?;
    }
    if kind.is_built_in() {
        // Fenix keeps the built-in's own and adds the markers: whatever
        // else is there would be ignored, so it's refused.
        for (field, given) in [("name", kind.name.is_some()), ("tag", kind.tag.is_some()), ("color", kind.color.is_some()), ("main", !kind.main.is_empty())] {
            if given {
                return Err(format!("{id} is one of Fenix's own: a pack adds its markers and nothing else, so no {field}"));
            }
        }
        return Ok(());
    }
    let name = kind.name.as_deref().ok_or("it needs a name")?;
    if name.is_empty() || name.chars().count() > MAX_NAME || name.chars().any(char::is_control) {
        return Err(format!("name {name:?} -- 1 to {MAX_NAME} characters, no control characters"));
    }
    let tag = kind.tag.as_deref().ok_or("it needs a tag")?;
    if tag.is_empty() || tag.len() > MAX_TAG || !tag.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, '+' | '#' | '.')) || !tag.chars().any(|c| c.is_ascii_uppercase()) {
        return Err(format!("tag {tag:?} -- 1 to {MAX_TAG} of A-Z 0-9 + # . with at least one letter"));
    }
    if let Some(color) = kind.color.as_deref().filter(|c| !COLORS.contains(c)) {
        return Err(format!("color {color:?} -- one of {}", COLORS.join(", ")));
    }
    Ok(())
}

/// A pack's kinds and its templates' kinds, held to what Fenix holds one
/// manifest to (the store-wide rules are `check_store`'s). `dir` is an
/// extension folder: here, or a package built elsewhere, unpacked.
pub fn check_pack(dir: &Path) -> Result<Pack, String> {
    let path = dir.join("extension.toml");
    let at = path.display();
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{at}: {e}"))?;
    let table: toml::Table = toml::from_str(&text).map_err(|e| format!("{at}: {}", e.message()))?;
    let string = |section: &str, key: &str| table.get(section).and_then(|s| s.get(key)).and_then(|v| v.as_str());
    let id = string("extension", "id").unwrap_or_default().to_string();
    let dependencies = table.get("dependencies").and_then(|d| d.as_table()).map(|d| d.keys().cloned().collect()).unwrap_or_default();
    let mut pack = Pack { id, kinds: Vec::new(), templates: Vec::new(), dependencies };

    if let Some(section) = table.get("contributes").and_then(|c| c.get("project_kinds")) {
        let array = section.as_array().ok_or_else(|| format!("{at}: contributes.project_kinds is a list of [[contributes.project_kinds]]"))?;
        if string("extension", "runtime") == Some("bundle") && !array.is_empty() {
            return Err(format!("{at}: a bundle has no contributions, project kinds among them"));
        }
        if array.len() > MAX_KINDS {
            return Err(format!("{at}: {} project kinds -- at most {MAX_KINDS} to a pack", array.len()));
        }
        for item in array {
            let kind: Declared = item.clone().try_into().map_err(|e: toml::de::Error| format!("{at}: project kind {}: {}", item.get("id").and_then(|i| i.as_str()).unwrap_or("?"), e.message()))?;
            check_kind(&kind).map_err(|e| format!("{at}: project kind {}: {e}", kind.id))?;
            if pack.kinds.iter().any(|k| k.id == kind.id) {
                return Err(format!("{at}: project kind {} is there twice", kind.id));
            }
            pack.kinds.push(kind);
        }
    }

    // A template names a kind. Fenix 1.5 refuses one it doesn't know, so
    // a template naming a pack's kind is for Fenix 1.6 and later.
    let floor = string("engine", "fenix").and_then(|r| semver::VersionReq::parse(r).ok()).map(|r| crate::fenix_theme::floor(&r)).unwrap_or(semver::Version::new(0, 0, 0));
    let templates = dir.join("templates");
    let mut folders: Vec<_> = std::fs::read_dir(&templates).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.join("template.toml").is_file()).collect();
    folders.sort();
    for folder in folders {
        let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let file = folder.join("template.toml");
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let parsed: toml::Table = toml::from_str(&text).map_err(|e| format!("{}: {}", file.display(), e.message()))?;
        let Some(kind) = parsed.get("template").and_then(|t| t.get("kind")).and_then(|k| k.as_str()) else { continue };
        let lower = kind.to_ascii_lowercase();
        if !fenix_id(&lower) {
            if !well_formed(kind) {
                return Err(format!("{}: kind {kind:?} isn't a kind id -- a-z first, then a-z 0-9 - _, at most {MAX_ID}", file.display()));
            }
            if floor < semver::Version::new(1, 6, 0) {
                return Err(format!(
                    "{}: kind {kind:?} isn't one of Fenix's, and Fenix 1.5 refuses such a template: engine.fenix is {}, and it needs >=1.6 (or keep kind = \"other\" until it does)",
                    file.display(),
                    string("engine", "fenix").unwrap_or("?")
                ));
            }
        }
        pack.templates.push((name, kind.to_string()));
    }
    Ok(pack)
}

/// What the whole store says: a kind has one pack, a tag one kind and
/// none of Fenix's, and a template's kind is one somebody declares.
/// Returns the warnings (two packs' kinds sharing a marker; a template
/// naming another pack's kind it doesn't depend on).
pub fn check_store(packs: &[Pack]) -> Result<Vec<String>, String> {
    let mut warnings = Vec::new();
    // A built-in id's markers are added by any pack that has them; every
    // other id has one pack.
    let mut owners: BTreeMap<&str, &Pack> = BTreeMap::new();
    let mut tags: BTreeMap<&str, (&Pack, &str)> = BTreeMap::new();
    for pack in packs {
        for kind in pack.kinds.iter().filter(|k| !k.is_built_in()) {
            if let Some(other) = owners.insert(&kind.id, pack) {
                return Err(format!("project kind {} is declared by {} and by {}: one pack to an id", kind.id, other.id, pack.id));
            }
            let tag = kind.tag.as_deref().unwrap_or_default();
            if FENIX_TAGS.contains(&tag) {
                return Err(format!("{}: project kind {}: tag {tag} is one of Fenix's own ({})", pack.id, kind.id, FENIX_TAGS.join(", ")));
            }
            if let Some((other, other_id)) = tags.insert(tag, (pack, &kind.id)) {
                return Err(format!("tag {tag} is on kind {other_id} of {} and on kind {} of {}: a tag marks one kind", other.id, kind.id, pack.id));
            }
        }
    }
    // The same marker on kinds of two packs: whichever Fenix puts first
    // takes the project.
    let mut by_marker: BTreeMap<&str, Vec<(&Pack, &str)>> = BTreeMap::new();
    for pack in packs {
        for kind in &pack.kinds {
            for marker in &kind.markers {
                by_marker.entry(marker).or_default().push((pack, &kind.id));
            }
        }
    }
    for (marker, claims) in by_marker {
        let mut ids: Vec<String> = claims.iter().filter(|(p, id)| claims.iter().any(|(q, other)| q.id != p.id && other != id)).map(|(p, id)| format!("{id} ({})", p.id)).collect();
        ids.sort();
        ids.dedup();
        if !ids.is_empty() {
            warnings.push(format!("project kinds share the marker {marker}: {} -- Fenix takes them in its own order", ids.join(", ")));
        }
    }
    for pack in packs {
        for (template, kind) in &pack.templates {
            let kind = kind.to_ascii_lowercase();
            if fenix_id(&kind) || pack.kinds.iter().any(|k| k.id == kind) {
                continue;
            }
            match owners.get(kind.as_str()) {
                Some(owner) if pack.dependencies.contains(&owner.id) => {}
                Some(owner) => warnings.push(format!("{}: template {template} names kind {kind}, which {} declares and {} doesn't depend on", pack.id, owner.id, pack.id)),
                None => return Err(format!("{}: template {template} names kind {kind}, and no pack in the store declares it", pack.id)),
            }
        }
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Temp {
            // Tests run side by side: a folder for each call.
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("xtask-kinds-{name}-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A pack folder with this `[engine]` extra and what follows it.
    fn pack_dir(t: &Temp, id: &str, fenix: &str, rest: &str) -> PathBuf {
        let dir = t.0.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("extension.toml"), format!("[extension]\nid = \"acme.{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\nruntime = \"none\"\n\n[engine]\nfenix = \"{fenix}\"\n{rest}")).unwrap();
        dir
    }

    fn java() -> String {
        "\n[[contributes.project_kinds]]\nid = \"java\"\nname = \"Java\"\ntag = \"JAVA\"\ncolor = \"keyword\"\nmarkers = [\"pom.xml\", \"{folder}.gradle\"]\nmain = [\"src/Main.java\", \"app/{folder}.java\"]\n".to_string()
    }

    fn refused(rest: &str, says: &str) {
        let t = Temp::new("refused");
        let dir = pack_dir(&t, "x", ">=1.5, <2", rest);
        let err = check_pack(&dir).unwrap_err();
        assert!(err.contains(says), "{rest:?}: {err}");
    }

    fn kind(fields: &str) -> String {
        format!("\n[[contributes.project_kinds]]\n{fields}\n")
    }

    #[test]
    fn a_kind_as_the_plan_writes_it_passes_with_no_api_and_any_runtime() {
        let t = Temp::new("fine");
        let dir = pack_dir(&t, "j", ">=1.5, <2", &java());
        let pack = check_pack(&dir).unwrap();
        assert_eq!(pack.kinds.len(), 1);
        assert_eq!(pack.kinds[0].markers, ["pom.xml", "{folder}.gradle"]);
        // Not tied to engine.api: with one, without one.
        std::fs::write(dir.join("extension.toml"), std::fs::read_to_string(dir.join("extension.toml")).unwrap().replace("[engine]\n", "[engine]\napi = \"1.5\"\n")).unwrap();
        check_pack(&dir).unwrap();
        // A built-in id adds markers, and only those.
        let dir = pack_dir(&t, "n", ">=1.5, <2", &kind("id = \"node\"\nmarkers = [\"deno.json\"]"));
        assert!(check_pack(&dir).unwrap().kinds[0].is_built_in());
        // Declared only: no markers.
        check_pack(&pack_dir(&t, "k", ">=1.5, <2", &kind("id = \"kotlin\"\nname = \"Kotlin\"\ntag = \"KT\""))).unwrap();
        for tag in ["C#", "C++X", "A.B", "F1"] {
            check_pack(&pack_dir(&t, "tags", ">=1.5, <2", &kind(&format!("id = \"cs\"\nname = \"C\"\ntag = \"{tag}\"")))).unwrap();
        }
    }

    #[test]
    fn ids_are_well_formed_and_not_fenixs() {
        let with = |id: &str| kind(&format!("id = \"{id}\"\nname = \"X\"\ntag = \"X\""));
        refused("\n[[contributes.project_kinds]]\nname = \"X\"\ntag = \"X\"\n", "id");
        for bad in ["", "Java", "1java", "-java", "ja va", "ja.va", "java/x", &"a".repeat(25)] {
            refused(&with(bad), "1 to 24 characters");
        }
        check_pack(&pack_dir(&Temp::new("ok24"), "x", ">=1.5, <2", &with(&"a".repeat(24)))).unwrap();
        check_pack(&pack_dir(&Temp::new("okchars"), "x", ">=1.5, <2", &with("a0-b_c"))).unwrap();
        for reserved in RESERVED_IDS.iter().filter(|r| well_formed(r)) {
            refused(&with(reserved), "is Fenix's own");
        }
        refused(&with("c++"), "1 to 24 characters");
        refused(&(with("java") + &with("java")), "there twice");
    }

    #[test]
    fn a_name_and_a_tag_are_needed_and_limited() {
        refused(&kind("id = \"java\"\ntag = \"JAVA\""), "needs a name");
        refused(&kind("id = \"java\"\nname = \"Java\""), "needs a tag");
        refused(&kind("id = \"java\"\nname = \"\"\ntag = \"J\""), "name");
        refused(&kind(&format!("id = \"java\"\nname = \"{}\"\ntag = \"J\"", "n".repeat(33))), "1 to 32 characters");
        refused(&kind("id = \"java\"\nname = \"Ja\\nva\"\ntag = \"J\""), "no control characters");
        check_pack(&pack_dir(&Temp::new("n32"), "x", ">=1.5, <2", &kind(&format!("id = \"java\"\nname = \"{}\"\ntag = \"J\"", "n".repeat(32))))).unwrap();
        for bad in ["", "JAVAX", "java", "Java", "J-A", "J A", "123", "++", "É", "J\\u0301"] {
            refused(&kind(&format!("id = \"java\"\nname = \"Java\"\ntag = \"{bad}\"")), "tag");
        }
    }

    #[test]
    fn a_colour_is_one_of_the_eight_roles() {
        for color in COLORS {
            check_pack(&pack_dir(&Temp::new("c"), "x", ">=1.5, <2", &kind(&format!("id = \"java\"\nname = \"Java\"\ntag = \"J\"\ncolor = \"{color}\"")))).unwrap();
        }
        for bad in ["#cc7832", "red", "Keyword", "muted", "fg", ""] {
            refused(&kind(&format!("id = \"java\"\nname = \"Java\"\ntag = \"J\"\ncolor = \"{bad}\"")), "one of keyword, string");
        }
    }

    #[test]
    fn markers_are_names_at_the_top() {
        let with = |marker: &str| kind(&format!("id = \"java\"\nname = \"Java\"\ntag = \"J\"\nmarkers = [\"{marker}\"]"));
        for bad in ["a/b", "a\\\\b", "..", "a..b", "*.java", "pom?", "x[1]", "{folder}{folder}", "{folder", "{other}", "a}", "x{folder}{", ".fenix", ".FENIX", ""] {
            refused(&with(bad), "marker");
        }
        refused(&with(&"m".repeat(65)), "65 bytes");
        refused(&with(&"é".repeat(33)), "66 bytes");
        for fine in ["pom.xml", "{folder}.sln", "x{folder}y", &"m".repeat(64), ".terraform.lock.hcl", ".fenixignore", "a.b."] {
            check_pack(&pack_dir(&Temp::new("m"), "x", ">=1.5, <2", &with(fine))).unwrap_or_else(|e| panic!("{fine}: {e}"));
        }
        let many = |n: usize| kind(&format!("id = \"java\"\nname = \"Java\"\ntag = \"J\"\nmarkers = [{}]", (0..n).map(|i| format!("\"m{i}\"")).collect::<Vec<_>>().join(", ")));
        check_pack(&pack_dir(&Temp::new("m16"), "x", ">=1.5, <2", &many(16))).unwrap();
        refused(&many(17), "17 markers -- at most 16");
    }

    #[test]
    fn main_files_are_paths_below_the_project() {
        let with = |main: &str| kind(&format!("id = \"java\"\nname = \"Java\"\ntag = \"J\"\nmain = [\"{main}\"]"));
        for bad in ["/etc/passwd", "a\\\\b", "../x", "a/../b", "a/..", "a//b", "a/", ""] {
            refused(&with(bad), "main");
        }
        refused(&with(&"p".repeat(129)), "129 bytes");
        for fine in ["src/Main.java", "lib/{folder}.ex", "Sources/{folder}/main.swift", &"p".repeat(128), "a..b/c"] {
            check_pack(&pack_dir(&Temp::new("main"), "x", ">=1.5, <2", &with(fine))).unwrap_or_else(|e| panic!("{fine}: {e}"));
        }
        let many = |n: usize| kind(&format!("id = \"java\"\nname = \"Java\"\ntag = \"J\"\nmain = [{}]", (0..n).map(|i| format!("\"m{i}\"")).collect::<Vec<_>>().join(", ")));
        check_pack(&pack_dir(&Temp::new("main8"), "x", ">=1.5, <2", &many(8))).unwrap();
        refused(&many(9), "9 main files -- at most 8");
    }

    #[test]
    fn a_built_in_id_takes_markers_and_nothing_else() {
        for field in ["name = \"Node\"", "tag = \"N\"", "color = \"keyword\"", "main = [\"index.js\"]"] {
            refused(&kind(&format!("id = \"node\"\n{field}")), "one of Fenix's own");
        }
        for id in BUILT_IN_IDS {
            check_pack(&pack_dir(&Temp::new("b"), "x", ">=1.5, <2", &kind(&format!("id = \"{id}\"\nmarkers = [\"a.b\"]")))).unwrap();
        }
    }

    #[test]
    fn a_pack_has_at_most_eight_kinds_and_a_bundle_none() {
        let kinds = |n: usize| (0..n).map(|i| kind(&format!("id = \"k{i}\"\nname = \"K\"\ntag = \"K{i}\""))).collect::<String>();
        check_pack(&pack_dir(&Temp::new("k8"), "x", ">=1.5, <2", &kinds(8))).unwrap();
        refused(&kinds(9), "9 project kinds -- at most 8");
        let t = Temp::new("bundle");
        let dir = pack_dir(&t, "b", ">=1.5, <2", &("\n[dependencies]\n\"acme.a\" = \"*\"\n".to_string() + &java()));
        std::fs::write(dir.join("extension.toml"), std::fs::read_to_string(dir.join("extension.toml")).unwrap().replace("\"none\"", "\"bundle\"")).unwrap();
        assert!(check_pack(&dir).unwrap_err().contains("a bundle has no contributions"));
    }

    #[test]
    fn a_typo_in_a_field_or_its_type_is_an_error() {
        refused(&kind("id = \"java\"\nname = \"Java\"\ntag = \"J\"\nmarker = [\"pom.xml\"]"), "unknown field `marker`");
        refused(&kind("id = \"java\"\nname = \"Java\"\ntag = \"J\"\nmarkers = \"pom.xml\""), "expected a sequence");
        refused(&kind("id = \"java\"\nname = 4\ntag = \"J\""), "java");
        refused("\n[contributes]\nproject_kinds = \"java\"\n", "list of");
    }

    fn pack(id: &str, kinds: &[(&str, &str, &[&str])], templates: &[&str]) -> Pack {
        Pack {
            id: id.into(),
            kinds: kinds
                .iter()
                .map(|(k, tag, markers)| Declared { id: k.to_string(), name: Some(k.to_string()), tag: Some(tag.to_string()), color: None, markers: markers.iter().map(|m| m.to_string()).collect(), main: Vec::new() })
                .collect(),
            templates: templates.iter().enumerate().map(|(i, k)| (format!("t{i}"), k.to_string())).collect(),
            dependencies: BTreeSet::new(),
        }
    }

    #[test]
    fn the_store_has_one_pack_to_an_id_and_one_kind_to_a_tag() {
        let a = pack("fenix.a", &[("java", "JAVA", &["pom.xml"])], &[]);
        assert_eq!(check_store(std::slice::from_ref(&a)).unwrap(), Vec::<String>::new());
        let same_id = pack("fenix.b", &[("java", "JV", &[])], &[]);
        let err = check_store(&[a.clone(), same_id]).unwrap_err();
        assert!(err.contains("java is declared by fenix.a and by fenix.b"), "{err}");
        let same_tag = pack("fenix.b", &[("jvm", "JAVA", &[])], &[]);
        let err = check_store(&[a.clone(), same_tag]).unwrap_err();
        assert!(err.contains("tag JAVA") && err.contains("fenix.a") && err.contains("fenix.b"), "{err}");
        let twice = pack("fenix.c", &[("x", "XX", &[]), ("y", "XX", &[])], &[]);
        assert!(check_store(&[twice]).unwrap_err().contains("tag XX"));
        for own in FENIX_TAGS {
            let err = check_store(&[pack("fenix.c", &[("zzz", own, &[])], &[])]).unwrap_err();
            assert!(err.contains("one of Fenix's own"), "{own}: {err}");
        }
        // Built-in ids are Fenix's: two packs may add markers to one.
        let node = |id: &str, m: &'static str| pack(id, &[("node", "", &[m])], &[]);
        assert!(check_store(&[node("fenix.web", "deno.json"), node("fenix.other", "bun.lockb")]).unwrap().is_empty());
    }

    #[test]
    fn two_packs_sharing_a_marker_is_a_warning() {
        let a = pack("fenix.a", &[("java", "JAVA", &["build.gradle", "pom.xml"])], &[]);
        let b = pack("fenix.b", &[("groovy", "GRV", &["build.gradle"])], &[]);
        let warnings = check_store(&[a.clone(), b]).unwrap();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("build.gradle") && warnings[0].contains("java (fenix.a)") && warnings[0].contains("groovy (fenix.b)"));
        // One kind adding to a built-in id twice, or one pack's two kinds: not two packs.
        let c = pack("fenix.c", &[("x", "XX", &["m"]), ("y", "YY", &["m"])], &[]);
        assert!(check_store(&[c]).unwrap().is_empty());
    }

    #[test]
    fn a_templates_kind_is_fenixs_its_packs_or_a_pack_the_store_has() {
        let java = pack("fenix.java", &[("java", "JAVA", &[])], &["java", "other", "rust", "py", "java"]);
        assert!(check_store(std::slice::from_ref(&java)).unwrap().is_empty());
        let typo = pack("fenix.t", &[], &["javaa"]);
        let err = check_store(&[java.clone(), typo]).unwrap_err();
        assert!(err.contains("template t0 names kind javaa") && err.contains("no pack in the store declares it"), "{err}");
        let other = pack("fenix.kt", &[], &["java"]);
        let warnings = check_store(&[java.clone(), other.clone()]).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("fenix.java declares and fenix.kt doesn't depend on"), "{warnings:?}");
        let mut depends = other;
        depends.dependencies.insert("fenix.java".into());
        assert!(check_store(&[java, depends]).unwrap().is_empty());
    }

    #[test]
    fn a_template_naming_a_new_kind_needs_fenix_1_6() {
        let t = Temp::new("tpl");
        let template = |dir: &Path, kind: &str| {
            std::fs::create_dir_all(dir.join("templates/app")).unwrap();
            std::fs::write(dir.join("templates/app/template.toml"), format!("[template]\nname = \"App\"\nkind = \"{kind}\"\n")).unwrap();
        };
        for kind in ["other", "rust", "python", "node", "go", "cpp", "arduino", "tcl", "monorepo", "py", "Rust"] {
            let dir = pack_dir(&t, "old", ">=1.5, <2", &java());
            template(&dir, kind);
            assert_eq!(check_pack(&dir).unwrap().templates, [("app".to_string(), kind.to_string())]);
        }
        let dir = pack_dir(&t, "old", ">=1.5, <2", &java());
        template(&dir, "java");
        let err = check_pack(&dir).unwrap_err();
        assert!(err.contains("Fenix 1.5 refuses") && err.contains(">=1.6"), "{err}");
        for floor in [">=1.6, <2", "^1.6", ">=1.6.0, <3", ">=2, <3"] {
            let dir = pack_dir(&t, "new", floor, &java());
            template(&dir, "java");
            assert_eq!(check_pack(&dir).unwrap().templates[0].1, "java", "{floor}");
        }
        let dir = pack_dir(&t, "new", ">=1.6, <2", &java());
        for bad in ["Java", "j a", "cobol/x", &"a".repeat(25)] {
            template(&dir, bad);
            assert!(check_pack(&dir).unwrap_err().contains("isn't a kind id"), "{bad}");
        }
    }

    /// Every pack in the store, as it is, passes both halves.
    #[test]
    fn the_store_s_packs_pass() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut packs = Vec::new();
        let mut dirs: Vec<_> = std::fs::read_dir(root.join("extensions")).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        dirs.sort();
        for dir in dirs {
            packs.push(check_pack(&dir).unwrap_or_else(|e| panic!("{e}")));
        }
        check_store(&packs).unwrap();
    }
}
