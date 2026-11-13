//! The extension API with the fake for its host: everything
//! `fenix_extension_api` has -- its types, `Page`'s builders, `export!`
//! -- and each call that would go to Fenix going to this thread's
//! [`Fake`](crate::Fake) instead.
//!
//! An extension's code takes it in its tests in place of the API, by the
//! name it already calls the API by:
//!
//! ```rust,ignore
//! #[cfg(not(test))]
//! use fenix_extension_api as api;
//! #[cfg(test)]
//! use fenix_test_kit::api;
//! ```
//!
//! Its `api::notify(..)`, `api::page::open(..)`, `api::host::set_timer(..)`
//! and `api::warn!(..)` are then the fake's in `cargo test`, and Fenix's in
//! the WebAssembly it packs, which has nothing of the kit in it. Every
//! free function and every `host` call of API 1.6 is here -- 1.0's, and
//! what 1.1 to 1.6 added: `key_for`, `key_scheme`, `notify_dismiss`,
//! `compose_with_ask`, `progress`, `host::notify_with`,
//! `host::git_state`, `host::last_task`, `tests::provider` (API 1.6's
//! `test-provider-set`) -- each checked to have the API's
//! own signature.
//!
//! What it can't replace are methods on the API's types --
//! `Command::spawn`, `HttpRequest::send`, `Notification::show` and the
//! rest, which call the host themselves. Natively those panic
//! (`unreachable`); call the host function they stand for instead,
//! `api::host::spawn(&cmd)`, `api::host::http(&req)`,
//! `api::host::notify_with(&notification)`, which is what they do. And a
//! call by the API's own path, `fenix_extension_api::notify`, is Fenix's:
//! it panics too.

pub use fenix_extension_api::*;


pub mod host;

pub use crate::{__test_kit_error as error, __test_kit_info as info, __test_kit_warn as warn};

/// Each of these is the API's own signature.
macro_rules! same {
    ($path:ident => $($name:ident: fn($($t:ty),*) $(-> $r:ty)?;)*) => {
        $(const _: [fn($($t),*) $(-> $r)?; 2] = [$name, ::fenix_extension_api::$path::$name];)*
    };
    ($($name:ident: fn($($t:ty),*) $(-> $r:ty)?;)*) => {
        $(const _: [fn($($t),*) $(-> $r)?; 2] = [$name, ::fenix_extension_api::$name];)*
    };
}

pub fn platform() -> PlatformInfo {
    host::platform()
}
pub fn http_cancel(request: u64) {
    host::http_cancel(request)
}
pub fn pick(title: &str, items: &[PickerItem]) -> u64 {
    host::show_picker(title, items)
}
pub fn pick_many(title: &str, items: &[PickerItem]) -> u64 {
    host::show_picker_many(title, items)
}
pub fn compose(title: &str, verb: &str, initial: &str, allow_empty: bool) -> u64 {
    host::compose(title, verb, initial, allow_empty)
}
pub fn here() -> Option<Here> {
    host::editor_here()
}
pub fn home(section: Option<&HomeSection>) {
    host::home_set(section)
}
pub fn answer(call: u64, result: Result<String, String>) {
    host::service_answer(call, result.as_deref().map_err(String::as_str))
}
pub fn call_service(name: &str, payload: &str) -> u64 {
    host::call_service(name, payload)
}
pub fn prompt(label: &str, initial: &str) -> u64 {
    host::prompt(label, initial)
}
pub fn prompt_secret(label: &str) -> u64 {
    host::prompt_secret(label)
}
pub fn secret(key: &str) -> Option<String> {
    host::secret(key)
}
pub fn set_secret(key: &str, value: Option<&str>) -> Result<(), String> {
    host::secret_set(key, value)
}
pub fn env_var(name: &str) -> Option<String> {
    host::env_var(name)
}
pub fn notify(message: &str) {
    host::notify(Level::Info, message)
}
pub fn setting(key: &str) -> Option<String> {
    host::setting(key)
}
pub fn theme() -> EditorTheme {
    host::theme_colors()
}
pub fn theme_palette() -> Vec<Rgb> {
    host::theme_palette()
}
pub fn set_setting(key: &str, value: Option<&str>) -> Result<(), String> {
    host::setting_set(key, value)
}
pub fn open_text(title: &str, text: &str, language: Option<&str>) {
    host::open_text(title, text, language)
}
pub fn open_url(url: &str) -> Result<(), String> {
    host::open_url(url)
}
pub fn process_id() -> u32 {
    host::process_id()
}
pub fn find_program(name: &str, hints: &[&str]) -> Option<String> {
    let hints: Vec<String> = hints.iter().map(|h| h.to_string()).collect();
    host::find_program(name, &hints)
}
pub fn new_project(args: &str) {
    host::new_project(args)
}
pub fn copy(text: &str) {
    host::clipboard_set(text)
}
pub fn copy_image(png: &[u8]) -> Result<(), String> {
    host::clipboard_set_image(png)
}
pub fn pasted_image() -> Option<Vec<u8>> {
    host::clipboard_image()
}
pub fn status(text: Option<&str>) {
    host::status_set(text)
}
pub fn pasted_text() -> Option<String> {
    host::clipboard_text()
}
pub fn read_file(path: &str) -> Result<Vec<u8>, String> {
    host::file_read(path)
}
pub fn explorer_selected() -> Option<String> {
    host::explorer_selected()
}
pub fn cursor() -> Option<CursorInfo> {
    host::editor_cursor()
}
pub fn folder_list(path: &str) -> Result<Vec<FolderEntry>, String> {
    host::folder_list(path)
}
pub fn play_sound(path: &str) -> Result<(), String> {
    host::play_sound(path)
}
pub fn open_text_beside(title: &str, text: &str, language: Option<&str>, width: f32) {
    host::open_text_beside(title, text, language, width)
}
pub fn compose_with_ask(title: &str, verb: &str, initial: &str, allow_empty: bool, ask: agents::Ask, over_initial: bool) -> u64 {
    host::compose_with_ask(title, verb, initial, allow_empty, &ask.stream(true).payload(), over_initial)
}
pub fn key_scheme() -> KeyScheme {
    host::editor_keys()
}
pub fn key_for(command: &str) -> Option<String> {
    host::key_for(command)
}
pub fn progress(call: u64, message: &str, fraction: Option<f32>) {
    host::service_progress(call, message, fraction)
}
pub fn notify_dismiss(tag: &str) {
    host::notify_dismiss(tag)
}

/// API 1.6's `tests` module -- the types of a test provider's exports --
/// with [`provider`](tests::provider) the fake's.
pub mod tests {
    pub use fenix_extension_api::tests::*;

    /// What this provider runs in the project now, and why it can't:
    /// `test-provider-set`, which the fake writes down
    /// ([`Fake::test_providers`](crate::Fake::test_providers)).
    pub fn provider(provider: &str, runner: &str, problem: Option<&str>) -> Result<(), String> {
        super::host::test_provider_set(provider, runner, problem)
    }

    const _: [fn(&str, &str, Option<&str>) -> Result<(), String>; 2] = [provider, ::fenix_extension_api::tests::provider];
}

same! {
    platform: fn() -> PlatformInfo;
    http_cancel: fn(u64);
    pick: fn(&str, &[PickerItem]) -> u64;
    pick_many: fn(&str, &[PickerItem]) -> u64;
    compose: fn(&str, &str, &str, bool) -> u64;
    here: fn() -> Option<Here>;
    home: fn(Option<&HomeSection>);
    answer: fn(u64, Result<String, String>);
    call_service: fn(&str, &str) -> u64;
    prompt: fn(&str, &str) -> u64;
    prompt_secret: fn(&str) -> u64;
    secret: fn(&str) -> Option<String>;
    set_secret: fn(&str, Option<&str>) -> Result<(), String>;
    env_var: fn(&str) -> Option<String>;
    notify: fn(&str);
    setting: fn(&str) -> Option<String>;
    theme: fn() -> EditorTheme;
    theme_palette: fn() -> Vec<Rgb>;
    set_setting: fn(&str, Option<&str>) -> Result<(), String>;
    open_text: fn(&str, &str, Option<&str>);
    open_url: fn(&str) -> Result<(), String>;
    process_id: fn() -> u32;
    find_program: fn(&str, &[&str]) -> Option<String>;
    new_project: fn(&str);
    copy: fn(&str);
    copy_image: fn(&[u8]) -> Result<(), String>;
    pasted_image: fn() -> Option<Vec<u8>>;
    status: fn(Option<&str>);
    pasted_text: fn() -> Option<String>;
    read_file: fn(&str) -> Result<Vec<u8>, String>;
    explorer_selected: fn() -> Option<String>;
    cursor: fn() -> Option<CursorInfo>;
    folder_list: fn(&str) -> Result<Vec<FolderEntry>, String>;
    play_sound: fn(&str) -> Result<(), String>;
    open_text_beside: fn(&str, &str, Option<&str>, f32);
    // 1.1 to 1.5.
    compose_with_ask: fn(&str, &str, &str, bool, agents::Ask, bool) -> u64;
    key_scheme: fn() -> KeyScheme;
    key_for: fn(&str) -> Option<String>;
    progress: fn(u64, &str, Option<f32>);
    notify_dismiss: fn(&str);
}

/// Laying out its pages, and opening them: `page::open`, `refresh`,
/// `close`, `focus` and `file` the fake's, the spans and `Page`'s builders
/// the API's.
pub mod page {
    use super::host;
    #[allow(unused_imports)]
    pub use fenix_extension_api::page::*;

    pub fn open(kind: &str, title: &str) -> u64 {
        host::page_open(kind, title)
    }
    pub fn open_beside(kind: &str, title: &str, width: f32) -> u64 {
        host::page_open_beside(kind, title, width)
    }
    pub fn refresh(page: u64) {
        host::page_refresh(page)
    }
    pub fn close(page: u64) {
        host::page_close(page)
    }
    pub fn focus(page: u64) {
        host::page_focus(page)
    }
    pub fn file(page: u64, path: Option<&str>) -> Result<(), String> {
        host::page_file(page, path)
    }

    same! { page =>
        open: fn(&str, &str) -> u64;
        open_beside: fn(&str, &str, f32) -> u64;
        refresh: fn(u64);
        close: fn(u64);
        focus: fn(u64);
        file: fn(u64, Option<&str>) -> Result<(), String>;
    }
}

pub mod task {
    use super::host;
    #[allow(unused_imports)]
    pub use fenix_extension_api::task::*;

    pub fn cancel(task: u64) {
        host::task_cancel(task)
    }
    pub fn show(task: u64) -> Result<(), String> {
        host::task_show(task)
    }

    same! { task =>
        cancel: fn(u64);
        show: fn(u64) -> Result<(), String>;
    }
}

pub mod image {
    use super::host;
    #[allow(unused_imports)]
    pub use fenix_extension_api::image::*;
    use fenix_extension_api::ImageInfo;

    pub fn load(bytes: &[u8]) -> Result<ImageInfo, String> {
        host::image_load(bytes)
    }
    pub fn file(path: &str) -> Result<ImageInfo, String> {
        host::image_file(path)
    }
    pub fn free(image: u32) {
        host::image_free(image)
    }

    same! { image =>
        load: fn(&[u8]) -> Result<ImageInfo, String>;
        file: fn(&str) -> Result<ImageInfo, String>;
        free: fn(u32);
    }
}

pub mod project {
    use super::host;
    #[allow(unused_imports)]
    pub use fenix_extension_api::project::*;
    use fenix_extension_api::{Check, Command, Task};

    fn strings(languages: &[&str]) -> Vec<String> {
        languages.iter().map(|l| l.to_string()).collect()
    }

    pub fn language_server(languages: &[&str], command: Option<&Command>) -> Result<(), String> {
        host::language_server_set(&strings(languages), command)
    }
    pub fn add_language_server(name: &str, languages: &[&str], command: Option<&Command>) -> Result<(), String> {
        host::language_server_add(name, &strings(languages), command)
    }
    pub fn debug_adapter(languages: &[&str], command: Option<&Command>) -> Result<(), String> {
        host::debug_adapter_set(&strings(languages), command)
    }
    pub fn formatter(languages: &[&str], command: Option<&Command>) -> Result<(), String> {
        host::formatter_set(&strings(languages), command, false)
    }
    pub fn formatter_keeping_output(languages: &[&str], command: Option<&Command>) -> Result<(), String> {
        host::formatter_set(&strings(languages), command, true)
    }
    pub fn debug_adapter_connect(languages: &[&str], command: Option<&Command>, port: u16) -> Result<(), String> {
        host::debug_adapter_connect(&strings(languages), command, port)
    }
    pub fn language_server_settings(name: &str, settings: Option<&fenix_extension_api::serde_json::Value>) -> Result<(), String> {
        host::language_server_settings(name, settings.map(|s| s.to_string()).as_deref())
    }
    pub fn restart_language_server() {
        host::language_server_restart()
    }
    pub fn doctor(checks: &[Check]) {
        host::doctor_set(checks)
    }
    pub fn tasks(tasks: &[Task]) -> Result<(), String> {
        host::tasks_set(tasks)
    }

    same! { project =>
        language_server: fn(&[&str], Option<&Command>) -> Result<(), String>;
        add_language_server: fn(&str, &[&str], Option<&Command>) -> Result<(), String>;
        debug_adapter: fn(&[&str], Option<&Command>) -> Result<(), String>;
        formatter: fn(&[&str], Option<&Command>) -> Result<(), String>;
        formatter_keeping_output: fn(&[&str], Option<&Command>) -> Result<(), String>;
        debug_adapter_connect: fn(&[&str], Option<&Command>, u16) -> Result<(), String>;
        language_server_settings: fn(&str, Option<&fenix_extension_api::serde_json::Value>) -> Result<(), String>;
        restart_language_server: fn();
        doctor: fn(&[Check]);
        tasks: fn(&[Task]) -> Result<(), String>;
    }
}

pub mod debug {
    use super::host;
    #[allow(unused_imports)]
    pub use fenix_extension_api::debug::*;
    use fenix_extension_api::{LaunchConfig, PickerItem};

    pub fn launches(launches: &[LaunchConfig]) -> Result<(), String> {
        host::launches_set(launches)
    }
    pub fn variable(name: &str, choices: &[PickerItem]) -> Result<(), String> {
        host::launch_variable_set(name, choices)
    }

    same! { debug =>
        launches: fn(&[LaunchConfig]) -> Result<(), String>;
        variable: fn(&str, &[PickerItem]) -> Result<(), String>;
    }
}

pub mod layer {
    use super::host;
    #[allow(unused_imports)]
    pub use fenix_extension_api::layer::*;
    use fenix_extension_api::{Item, LayerLevel, ModelInfo};

    pub fn sheet(path: &str, frame: (u32, u32)) -> Result<u32, String> {
        host::sheet_load(path, frame.0, frame.1)
    }
    pub fn model(path: &str) -> Result<ModelInfo, String> {
        host::model_load(path)
    }
    pub fn open(level: LayerLevel) -> Result<u64, String> {
        host::layer_open(level)
    }
    pub fn set(layer: u64, items: &[Item]) {
        host::layer_set(layer, items)
    }
    pub fn close(layer: u64) {
        host::layer_close(layer)
    }

    same! { layer =>
        sheet: fn(&str, (u32, u32)) -> Result<u32, String>;
        model: fn(&str) -> Result<ModelInfo, String>;
        open: fn(LayerLevel) -> Result<u64, String>;
        set: fn(u64, &[Item]);
        close: fn(u64);
    }
}

pub mod terminal {
    use super::host;
    #[allow(unused_imports)]
    pub use fenix_extension_api::terminal::*;
    use fenix_extension_api::Command;

    pub fn stop(terminal: u64) {
        host::terminal_stop(terminal)
    }
    pub fn restart(terminal: u64, command: Option<&Command>, title: Option<&str>) -> Result<(), String> {
        host::terminal_restart(terminal, command, title)
    }

    same! { terminal =>
        stop: fn(u64);
        restart: fn(u64, Option<&Command>, Option<&str>) -> Result<(), String>;
    }
}

/// The time where you are: the fake's, when a test set it
/// ([`Fake::clock`](crate::Fake::clock)), else this machine's.
pub mod clock {
    #[allow(unused_imports)]
    pub use fenix_extension_api::clock::*;
    use fenix_extension_api::chrono::{DateTime, FixedOffset};

    pub fn now() -> DateTime<FixedOffset> {
        crate::Fake::with(|s| s.clock).unwrap_or_else(fenix_extension_api::clock::now)
    }

    same! { clock =>
        now: fn() -> DateTime<FixedOffset>;
    }
}

pub mod buffer {
    use super::host;
    #[allow(unused_imports)]
    pub use fenix_extension_api::buffer::*;
    use fenix_extension_api::{TextBuffer, TextEdit};

    pub fn text(path: &str) -> Option<String> {
        host::buffer_text(path)
    }
    pub fn edit(path: &str, edits: &[TextEdit]) -> Result<(), String> {
        host::buffer_edit(path, edits)
    }
    pub fn insert(text: &str) -> Result<(), String> {
        host::buffer_insert(text)
    }
    pub fn save(path: &str) -> Result<(), String> {
        host::buffer_save(path)
    }
    pub fn set_language(buffer: &TextBuffer, language: Option<&str>) -> Result<(), String> {
        host::buffer_language_set(buffer, language)
    }
    pub fn undo(path: &str) -> Result<(), String> {
        host::buffer_undo(path)
    }
    pub fn redo(path: &str) -> Result<(), String> {
        host::buffer_redo(path)
    }

    same! { buffer =>
        text: fn(&str) -> Option<String>;
        edit: fn(&str, &[TextEdit]) -> Result<(), String>;
        insert: fn(&str) -> Result<(), String>;
        save: fn(&str) -> Result<(), String>;
        set_language: fn(&TextBuffer, Option<&str>) -> Result<(), String>;
        undo: fn(&str) -> Result<(), String>;
        redo: fn(&str) -> Result<(), String>;
    }
}

pub mod decorations {
    use super::host;
    #[allow(unused_imports)]
    pub use fenix_extension_api::decorations::*;
    use fenix_extension_api::{Decoration, DecorationRule, TextBuffer};

    pub fn set(buffer: &TextBuffer, decorations: &[Decoration]) -> Result<(), String> {
        host::decorations_set(buffer, decorations)
    }
    pub fn rules(rules: &[DecorationRule]) -> Result<(), String> {
        host::decoration_rules_set(rules)
    }

    same! { decorations =>
        set: fn(&TextBuffer, &[Decoration]) -> Result<(), String>;
        rules: fn(&[DecorationRule]) -> Result<(), String>;
    }
}

pub mod scratch {
    use super::host;
    #[allow(unused_imports)]
    pub use fenix_extension_api::scratch::*;

    pub fn open(title: &str, text: &str, language: Option<&str>) -> u64 {
        host::scratch_open(title, text, language)
    }
    pub fn open_beside(title: &str, text: &str, language: Option<&str>, width: f32) -> u64 {
        host::scratch_open_beside(title, text, language, width)
    }
    pub fn text(scratch: u64) -> Option<String> {
        host::scratch_text(scratch)
    }
    pub fn set(scratch: u64, text: &str) -> Result<(), String> {
        host::scratch_set(scratch, text)
    }
    pub fn close(scratch: u64) {
        host::scratch_close(scratch)
    }

    same! { scratch =>
        open: fn(&str, &str, Option<&str>) -> u64;
        open_beside: fn(&str, &str, Option<&str>, f32) -> u64;
        text: fn(u64) -> Option<String>;
        set: fn(u64, &str) -> Result<(), String>;
        close: fn(u64);
    }
}

pub mod process {
    use super::host;
    #[allow(unused_imports)]
    pub use fenix_extension_api::process::*;

    pub fn write(process: u64, data: &[u8]) -> Result<(), String> {
        host::write_stdin(process, data)
    }
    pub fn close_stdin(process: u64) {
        host::close_stdin(process)
    }
    pub fn kill(process: u64) {
        host::kill(process)
    }

    same! { process =>
        write: fn(u64, &[u8]) -> Result<(), String>;
        close_stdin: fn(u64);
        kill: fn(u64);
    }
}

pub mod diagnostics {
    use super::host;
    #[allow(unused_imports)]
    pub use fenix_extension_api::diagnostics::*;
    use fenix_extension_api::Problem;

    pub fn set(path: &str, problems: &[Problem]) -> Result<(), String> {
        host::diagnostics_set(path, problems)
    }

    same! { diagnostics =>
        set: fn(&str, &[Problem]) -> Result<(), String>;
    }
}

pub mod tools {
    use super::host;
    #[allow(unused_imports)]
    pub use fenix_extension_api::tools::*;

    pub fn set_executable(path: &str) -> Result<(), String> {
        host::set_executable(path)
    }

    same! { tools =>
        set_executable: fn(&str) -> Result<(), String>;
    }
}

pub mod agents {
    use super::host;
    #[allow(unused_imports)]
    pub use fenix_extension_api::agents::*;
    use fenix_extension_api::Command;

    pub fn provide(id: &str, command: Option<&Command>) -> Result<(), String> {
        host::agent_provide(id, command)
    }

    same! { agents =>
        provide: fn(&str, Option<&Command>) -> Result<(), String>;
    }
}
