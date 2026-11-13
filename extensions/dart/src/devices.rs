//! Flutter's devices, as `flutter devices --machine` lists them: what the
//! `flutter run` picker offers, and a launch configuration for each, so a
//! debug session can start on a device by name.

use fenix_extension_api::{LaunchConfig, PickerItem};
use fenix_lang_kit::serde_json::{self, json, Value};
use fenix_lang_kit::Output;

/// A device Flutter can run an app on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// `android-arm64`, `linux-x64`, `web-javascript`, `ios`.
    pub platform: String,
    pub emulator: bool,
    /// The operating system, or the browser, as Flutter says it.
    pub sdk: String,
}

impl Device {
    /// `Pixel 8 · android-arm64`.
    pub fn label(&self) -> String {
        let kind = if self.emulator { format!("{} (emulator)", self.platform) } else { self.platform.clone() };
        format!("{} · {kind}", self.name)
    }
}

/// The devices `flutter devices --machine` printed. The JSON is an array
/// on stdout; Flutter may say something first (a lock it's waiting for,
/// that it's updating), so the array is read from its first line that
/// starts one. A device Flutter says it doesn't support is left out.
pub fn parse(out: &Output) -> Result<Vec<Device>, String> {
    let start = out.stdout.lines().scan(0, |at, l| {
        let here = *at;
        *at += l.len() + 1;
        Some((here, l))
    });
    let from = start.filter(|(_, l)| l.trim_start().starts_with('[')).map(|(at, _)| at).next();
    let Some(from) = from else {
        return Err(if out.ok() { "flutter devices printed no list".into() } else { out.why() });
    };
    let list: Vec<Value> = serde_json::from_str(out.stdout[from..].trim()).map_err(|e| format!("flutter devices --machine: {e}"))?;
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    Ok(list
        .iter()
        .filter(|d| d.get("isSupported").and_then(Value::as_bool) != Some(false))
        .map(|d| Device { id: text(d, "id"), name: text(d, "name"), platform: text(d, "targetPlatform"), emulator: d.get("emulator").and_then(Value::as_bool) == Some(true), sdk: text(d, "sdk") })
        .filter(|d| !d.id.is_empty())
        .collect())
}

/// The picker's rows: the device's name, its platform and system beside.
pub fn items(devices: &[Device]) -> Vec<PickerItem> {
    devices.iter().map(|d| PickerItem::new(d.name.clone(), d.id.clone()).detail(format!("{} · {}", if d.emulator { format!("{} emulator", d.platform) } else { d.platform.clone() }, d.sdk))).collect()
}

/// A debug session on each device: Flutter's adapter, `lib/main.dart`,
/// `-d <id>`. Named by the device, so two Pixels still differ by their id.
pub fn launches(devices: &[Device]) -> Vec<LaunchConfig> {
    let mut seen: Vec<&str> = Vec::new();
    devices
        .iter()
        .map(|d| {
            let twin = seen.contains(&d.name.as_str());
            seen.push(&d.name);
            let name = if twin { format!("Flutter: run on {} ({})", d.name, d.id) } else { format!("Flutter: run on {}", d.name) };
            LaunchConfig::launch(&name, &["dart"]).program("/project/lib/main.dart").attributes(&json!({ "toolArgs": ["-d", d.id] }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    #[test]
    fn the_list_is_read_past_what_flutter_says_first() {
        let devices = parse(&said(include_str!("samples/flutter-devices.json"))).unwrap();
        let ids: Vec<&str> = devices.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["emulator-5554", "37271FDJG0023A", "linux", "chrome"], "the one it doesn't support is left out");
        assert_eq!(devices[0].label(), "sdk gphone64 x86 64 · android-x64 (emulator)");
        assert_eq!(devices[3].label(), "Chrome (web) · web-javascript");
        assert_eq!(parse(&said("[]")).unwrap(), []);
    }

    #[test]
    fn what_isnt_a_list_says_why() {
        assert_eq!(parse(&said("No devices detected.\n")).unwrap_err(), "flutter devices printed no list");
        let failed = Output { code: Some(1), stdout: String::new(), stderr: "flutter: command not found".into() };
        assert_eq!(parse(&failed).unwrap_err(), "flutter: command not found");
        assert!(parse(&said("[ {\"id\": ")).unwrap_err().starts_with("flutter devices --machine:"));
    }

    #[test]
    fn the_picker_and_the_launches_name_each_device() {
        let devices = parse(&said(include_str!("samples/flutter-devices.json"))).unwrap();
        let rows = items(&devices);
        assert_eq!((rows[2].label.as_str(), rows[2].value.as_str(), rows[2].detail.as_str()), ("Linux (desktop)", "linux", "linux-x64 · Ubuntu 24.04.1 LTS 6.8.0-45-generic"));
        assert!(rows[0].detail.starts_with("android-x64 emulator"));
        let l = launches(&devices);
        assert_eq!(l[3].name, "Flutter: run on Chrome (web)");
        assert_eq!((l[3].languages.as_slice(), l[3].program.as_deref()), (["dart".to_string()].as_slice(), Some("/project/lib/main.dart")));
        assert_eq!(l[3].attributes, r#"{"toolArgs":["-d","chrome"]}"#);
        let twins = [devices[1].clone(), Device { id: "other".into(), ..devices[1].clone() }];
        assert_eq!(launches(&twins).iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Flutter: run on Pixel 8", "Flutter: run on Pixel 8 (other)"]);
    }
}
