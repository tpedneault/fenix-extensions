//! Arduino's side of things, as data: `arduino-cli ... --format json`
//! read into boards, ports, options, libraries and packages; a sketch's
//! own settings read and written where Arduino's tools keep them. No
//! host in here, so it's all tested natively.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The board a sketch that hasn't picked one is built for.
pub const DEFAULT_BOARD: &str = "arduino:avr:uno";

/// The monitor's speed until one is chosen.
pub const DEFAULT_BAUD: u32 = 9600;

/// Speeds the monitor offers, the same list the Arduino IDE has.
pub const BAUD_RATES: [u32; 12] = [300, 600, 750, 1200, 2400, 4800, 9600, 19200, 31250, 38400, 57600, 115200];

/// A board it can be built for: `id` is its FQBN (`arduino:avr:uno`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    pub id: String,
    pub name: String,
    /// The package it comes from, for grouping.
    pub package: String,
}

/// A port a board can be on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Port {
    /// `COM3`, `/dev/ttyACM0`.
    pub address: String,
    pub label: String,
    pub protocol: String,
    /// Boards recognized on it.
    pub boards: Vec<Board>,
}

/// One of a board's option menus ("Processor"), with its values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardOption {
    pub id: String,
    pub label: String,
    pub values: Vec<OptionValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionValue {
    pub value: String,
    pub label: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    pub name: String,
    pub latest: String,
    pub author: String,
    pub summary: String,
    pub installed: Option<String>,
}

/// Board support: a core, such as `arduino:avr`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub latest: String,
    pub installed: Option<String>,
}

fn parse(json: &str) -> Result<Value, String> {
    serde_json::from_str(json.trim_start_matches('\u{feff}')).map_err(|err| format!("unexpected arduino-cli output: {err}"))
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn array<'a>(v: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    v.get(key).and_then(Value::as_array).into_iter().flatten()
}

/// The package part of an FQBN: `arduino:avr:uno` -> `arduino:avr`.
pub fn package_of(fqbn: &str) -> String {
    fqbn.splitn(3, ':').take(2).collect::<Vec<_>>().join(":")
}

/// `arduino:avr:nano:cpu=atmega328old` -> `nano`.
pub fn short_board(board: &str) -> &str {
    board.split(':').nth(2).unwrap_or(board)
}

fn board(v: &Value) -> Option<Board> {
    let id = str_of(v, "fqbn")?.to_string();
    Some(Board { name: str_of(v, "name").unwrap_or(&id).to_string(), package: package_of(&id), id })
}

/// `board list`.
pub fn ports(json: &str) -> Result<Vec<Port>, String> {
    let v = parse(json)?;
    Ok(array(&v, "detected_ports")
        .filter_map(|detected| {
            let port = detected.get("port")?;
            let address = str_of(port, "address")?.to_string();
            Some(Port {
                label: str_of(port, "label").unwrap_or(&address).to_string(),
                protocol: str_of(port, "protocol_label").or_else(|| str_of(port, "protocol")).unwrap_or_default().to_string(),
                boards: array(detected, "matching_boards").filter_map(board).collect(),
                address,
            })
        })
        .collect())
}

/// `board listall`.
pub fn boards(json: &str) -> Result<Vec<Board>, String> {
    let v = parse(json)?;
    let mut boards: Vec<Board> = array(&v, "boards").filter_map(board).collect();
    boards.sort_by(|a, b| a.package.cmp(&b.package).then_with(|| a.name.cmp(&b.name)));
    Ok(boards)
}

/// `board details`'s `config_options`.
pub fn board_options(json: &str) -> Result<Vec<BoardOption>, String> {
    let v = parse(json)?;
    Ok(array(&v, "config_options")
        .filter_map(|option| {
            Some(BoardOption {
                id: str_of(option, "option")?.to_string(),
                label: str_of(option, "option_label").or_else(|| str_of(option, "option"))?.to_string(),
                values: array(option, "values")
                    .filter_map(|value| {
                        let id = str_of(value, "value")?.to_string();
                        Some(OptionValue { label: str_of(value, "value_label").unwrap_or(&id).to_string(), selected: value.get("selected").and_then(Value::as_bool).unwrap_or(false), value: id })
                    })
                    .collect(),
            })
        })
        .collect())
}

/// `lib search` joined with `lib list` (which knows what's installed).
pub fn libraries(search_json: &str, installed_json: &str) -> Result<Vec<Library>, String> {
    let search = parse(search_json)?;
    let installed = parse(installed_json)?;
    let installed: Vec<(String, String)> = array(&installed, "installed_libraries")
        .filter_map(|entry| {
            let lib = entry.get("library")?;
            Some((str_of(lib, "name")?.to_string(), str_of(lib, "version").unwrap_or_default().to_string()))
        })
        .collect();
    Ok(array(&search, "libraries")
        .filter_map(|lib| {
            let name = str_of(lib, "name")?.to_string();
            let latest = lib.get("latest");
            let field = |key: &str| latest.and_then(|l| str_of(l, key)).unwrap_or_default().to_string();
            Some(Library { installed: installed.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone()), latest: field("version"), author: field("author"), summary: field("sentence"), name })
        })
        .collect())
}

/// `core search` and `core list`.
pub fn packages(json: &str) -> Result<Vec<Package>, String> {
    let v = parse(json)?;
    Ok(array(&v, "platforms")
        .filter_map(|platform| {
            let id = str_of(platform, "id")?.to_string();
            let latest = str_of(platform, "latest_version").unwrap_or_default().to_string();
            let installed = str_of(platform, "installed_version").filter(|s| !s.is_empty()).map(str::to_string);
            let release = platform.get("releases").and_then(|r| r.get(&latest));
            Some(Package { name: release.and_then(|r| str_of(r, "name")).unwrap_or(&id).to_string(), id, latest, installed })
        })
        .collect())
}

/// `fqbn` with `option` set to `value`, every other option kept:
/// `arduino:avr:nano` + cpu/atmega328old -> `arduino:avr:nano:cpu=atmega328old`.
pub fn with_option(fqbn: &str, option: &str, value: &str) -> String {
    let mut parts = fqbn.splitn(4, ':');
    let base: Vec<&str> = parts.by_ref().take(3).collect();
    let mut options: Vec<(String, String)> = parts.next().unwrap_or_default().split(',').filter_map(|pair| pair.split_once('=').map(|(k, v)| (k.to_string(), v.to_string()))).collect();
    match options.iter_mut().find(|(k, _)| k == option) {
        Some(entry) => entry.1 = value.to_string(),
        None => options.push((option.to_string(), value.to_string())),
    }
    let options: Vec<String> = options.into_iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{}:{}", base.join(":"), options.join(","))
}

/// Whether `dir` is a sketch folder: a `sketch.yaml`, or a main `.ino`
/// (`.pde`) named after the folder.
pub fn is_sketch(dir: &Path) -> bool {
    if dir.join("sketch.yaml").is_file() {
        return true;
    }
    let Some(name) = dir.file_name().and_then(|n| n.to_str()) else { return false };
    dir.join(format!("{name}.ino")).is_file() || dir.join(format!("{name}.pde")).is_file()
}

/// The sketch `path` (a file or a folder) is in: the nearest folder that
/// is one, no higher than `top`. `top` itself (`/project`) has lost its
/// own name in the sandbox, so there an `.ino` (or `.pde`) of any name
/// will do: Fenix took the folder for a sketch by its real one.
pub fn sketch_of(path: &Path, top: &Path) -> Option<PathBuf> {
    let start = if path.is_dir() { path } else { path.parent()? };
    let has_main = |dir: &Path| std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| e.path().extension().is_some_and(|x| x == "ino" || x == "pde"));
    start.ancestors().take_while(|dir| dir.starts_with(top)).find(|dir| is_sketch(dir) || (*dir == top && has_main(dir))).map(Path::to_path_buf)
}

/// A top-level `key: value` from `sketch.yaml`, quotes stripped.
pub fn yaml_value(root: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(root.join("sketch.yaml")).ok()?;
    text.lines().find_map(|line| {
        let rest = line.strip_prefix(key)?.strip_prefix(':')?;
        let value = rest.trim().trim_matches(|c| c == '"' || c == '\'');
        (!value.is_empty()).then(|| value.to_string())
    })
}

/// Sets a top-level `key: value` in `sketch.yaml`, keeping the rest
/// (profiles, comments) as it was; makes the file if need be.
pub fn set_yaml_value(root: &Path, key: &str, value: &str) -> Result<(), String> {
    let path = root.join("sketch.yaml");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let line = format!("{key}: {value}");
    let mut replaced = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| {
            if !replaced && l.strip_prefix(key).is_some_and(|rest| rest.starts_with(':')) {
                replaced = true;
                line.clone()
            } else {
                l.to_string()
            }
        })
        .collect();
    if !replaced {
        lines.push(line);
    }
    let mut out = lines.join("\n");
    out.push('\n');
    std::fs::write(&path, out).map_err(|err| format!("couldn't write sketch.yaml: {err}"))
}

/// The sketch's board, and whether it chose it (`false`: the default).
pub fn board_of(root: &Path) -> (String, bool) {
    match yaml_value(root, "default_fqbn") {
        Some(fqbn) => (fqbn, true),
        None => (DEFAULT_BOARD.to_string(), false),
    }
}

pub fn port_of(root: &Path) -> Option<String> {
    yaml_value(root, "default_port")
}

/// `[monitor] baudrate` from the sketch's `.fenix/settings.toml`, or the
/// `.fenix/project.ini` it was kept in before.
pub fn baud_rate(root: &Path) -> u32 {
    let settings = std::fs::read_to_string(root.join(".fenix/settings.toml")).ok().and_then(|t| t.parse::<toml_edit::DocumentMut>().ok());
    if let Some(baud) = settings.as_ref().and_then(|d| d.get("monitor")?.get("baudrate")?.as_integer()) {
        return baud as u32;
    }
    let Ok(text) = std::fs::read_to_string(root.join(".fenix/project.ini")) else { return DEFAULT_BAUD };
    let mut in_monitor = false;
    for line in text.lines().map(str::trim) {
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_monitor = header.trim() == "monitor";
        } else if in_monitor {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "baudrate" {
                    return value.trim().parse().unwrap_or(DEFAULT_BAUD);
                }
            }
        }
    }
    DEFAULT_BAUD
}

/// Sets `[monitor] baudrate` in the sketch's `.fenix/settings.toml`,
/// leaving everything else in it as it was.
pub fn set_baud_rate(root: &Path, baud: u32) -> Result<(), String> {
    let path = root.join(".fenix/settings.toml");
    let mut doc = match std::fs::read_to_string(&path) {
        Ok(text) => text.parse::<toml_edit::DocumentMut>().map_err(|e| format!(".fenix/settings.toml can't be read, so it wasn't written over: {}", e.message().trim()))?,
        Err(_) => toml_edit::DocumentMut::new(),
    };
    if !doc.contains_table("monitor") {
        doc["monitor"] = toml_edit::table();
    }
    doc["monitor"]["baudrate"] = toml_edit::value(baud as i64);
    std::fs::create_dir_all(root.join(".fenix")).map_err(|e| e.to_string())?;
    std::fs::write(&path, doc.to_string()).map_err(|e| format!("couldn't write .fenix/settings.toml: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sketch(dir: &Path, name: &str) -> PathBuf {
        let root = dir.join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(format!("{name}.ino")), "void setup() {}\nvoid loop() {}\n").unwrap();
        root
    }

    #[test]
    fn a_folder_is_a_sketch_when_its_ino_matches_its_name_or_it_has_a_sketch_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let blink = sketch(dir.path(), "Blink");
        assert!(is_sketch(&blink));
        std::fs::create_dir_all(blink.join("src")).unwrap();
        assert_eq!(sketch_of(&blink.join("src/util.cpp"), dir.path()), Some(blink.clone()));
        assert_eq!(sketch_of(&blink.join("src/util.cpp"), &blink.join("src")), None, "not above the project");
        // The project's own folder, by another name: its .ino will do.
        let renamed = dir.path().join("project");
        std::fs::create_dir_all(&renamed).unwrap();
        std::fs::write(renamed.join("Blink.ino"), "").unwrap();
        assert_eq!(sketch_of(&renamed.join("Blink.ino"), &renamed), Some(renamed.clone()));
        assert_eq!(sketch_of(&renamed.join("Blink.ino"), dir.path()), None, "below the project, the name counts");
        let loose = dir.path().join("Loose");
        std::fs::create_dir_all(&loose).unwrap();
        std::fs::write(loose.join("other.ino"), "").unwrap();
        assert!(!is_sketch(&loose), "the main .ino is named after its folder");
        std::fs::write(loose.join("sketch.yaml"), "default_fqbn: arduino:avr:uno\n").unwrap();
        assert!(is_sketch(&loose));
    }

    #[test]
    fn its_board_port_and_speed_are_kept_where_arduino_keeps_them() {
        let dir = tempfile::tempdir().unwrap();
        let root = sketch(dir.path(), "Blink");
        assert_eq!(board_of(&root), (DEFAULT_BOARD.to_string(), false));
        assert_eq!(baud_rate(&root), DEFAULT_BAUD);
        std::fs::write(root.join("sketch.yaml"), "profiles:\n  uno:\n    fqbn: arduino:avr:uno\ndefault_fqbn: \"arduino:avr:uno\"\n").unwrap();
        set_yaml_value(&root, "default_fqbn", "arduino:avr:nano:cpu=atmega328old").unwrap();
        set_yaml_value(&root, "default_port", "COM4").unwrap();
        let text = std::fs::read_to_string(root.join("sketch.yaml")).unwrap();
        assert!(text.starts_with("profiles:\n  uno:\n    fqbn: arduino:avr:uno\n"), "{text}");
        assert_eq!(board_of(&root), ("arduino:avr:nano:cpu=atmega328old".to_string(), true));
        assert_eq!(port_of(&root).as_deref(), Some("COM4"));
        std::fs::create_dir_all(root.join(".fenix")).unwrap();
        std::fs::write(root.join(".fenix/settings.toml"), "[project]\nkind = \"arduino\"\n").unwrap();
        set_baud_rate(&root, 115200).unwrap();
        set_baud_rate(&root, 57600).unwrap();
        let text = std::fs::read_to_string(root.join(".fenix/settings.toml")).unwrap();
        assert_eq!(baud_rate(&root), 57600);
        assert!(text.contains("[project]\nkind = \"arduino\""), "{text}");
        assert_eq!(text.matches("baudrate").count(), 1);
    }

    #[test]
    fn a_sketch_not_yet_moved_still_has_its_old_speed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".fenix")).unwrap();
        std::fs::write(dir.path().join(".fenix/project.ini"), "[monitor]\nbaudrate = 115200\n").unwrap();
        assert_eq!(baud_rate(dir.path()), 115200);
    }

    #[test]
    fn with_option_adds_or_replaces_one_option() {
        assert_eq!(with_option("arduino:avr:nano", "cpu", "atmega328old"), "arduino:avr:nano:cpu=atmega328old");
        assert_eq!(with_option("arduino:avr:nano:cpu=atmega328", "cpu", "atmega328old"), "arduino:avr:nano:cpu=atmega328old");
        assert_eq!(with_option("esp32:esp32:esp32:PSRAM=disabled", "FlashSize", "4M"), "esp32:esp32:esp32:PSRAM=disabled,FlashSize=4M");
        assert_eq!(short_board("arduino:avr:nano:cpu=atmega328old"), "nano");
    }

    #[test]
    fn ports_carry_the_boards_recognized_on_them() {
        let json = r#"{"detected_ports": [
            {"port": {"address": "COM1", "label": "COM1", "protocol": "serial", "protocol_label": "Serial Port", "properties": {}}},
            {"matching_boards": [{"name": "Arduino UNO", "fqbn": "arduino:avr:uno"}],
             "port": {"address": "COM4", "label": "COM4", "protocol": "serial", "protocol_label": "Serial Port (USB)"}}
        ]}"#;
        let ports = ports(json).unwrap();
        assert_eq!(ports.len(), 2);
        assert!(ports[0].boards.is_empty());
        assert_eq!((ports[1].address.as_str(), ports[1].protocol.as_str()), ("COM4", "Serial Port (USB)"));
        assert_eq!(ports[1].boards[0], Board { id: "arduino:avr:uno".into(), name: "Arduino UNO".into(), package: "arduino:avr".into() });
        assert!(super::ports("\u{feff}{}").unwrap().is_empty());
    }

    #[test]
    fn boards_options_libraries_and_packages_are_read() {
        let names: Vec<String> = boards(r#"{"boards": [{"name": "Arduino Nano", "fqbn": "arduino:avr:nano"}, {"name": "Arduino Mega", "fqbn": "arduino:avr:mega"}, {"name": "no fqbn"}]}"#).unwrap().into_iter().map(|b| b.name).collect();
        assert_eq!(names, ["Arduino Mega", "Arduino Nano"]);
        let options = board_options(r#"{"config_options": [{"option": "cpu", "option_label": "Processor", "values": [{"value": "atmega328", "value_label": "ATmega328P", "selected": true}, {"value": "atmega328old"}]}]}"#).unwrap();
        assert_eq!((options[0].label.as_str(), options[0].values[0].selected, options[0].values[1].label.as_str()), ("Processor", true, "atmega328old"));
        let libs = libraries(r#"{"libraries": [{"name": "Servo", "latest": {"version": "1.3.0", "author": "Arduino", "sentence": "Servos."}}, {"name": "DHT"}]}"#, r#"{"installed_libraries": [{"library": {"name": "Servo", "version": "1.2.2"}}]}"#).unwrap();
        assert_eq!((libs[0].installed.as_deref(), libs[0].latest.as_str(), libs[1].installed.as_deref()), (Some("1.2.2"), "1.3.0", None));
        let packages = packages(r#"{"platforms": [{"id": "arduino:avr", "installed_version": "1.8.8", "latest_version": "1.8.8", "releases": {"1.8.8": {"name": "Arduino AVR Boards"}}}, {"id": "arduino:megaavr", "latest_version": "1.8.9"}]}"#).unwrap();
        assert_eq!((packages[0].name.as_str(), packages[0].installed.as_deref(), packages[1].installed.as_deref()), ("Arduino AVR Boards", Some("1.8.8"), None));
    }
}
