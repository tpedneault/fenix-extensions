//! `host`: every call of the API's host interface, made of the fake.
//!
//! Each has the signature of the API's own (checked at the foot of the
//! file), so code written against `api::host::...` builds both ways. What
//! the fake keeps it keeps (see [`State`](crate::fake::State)); the rest
//! is written down in the log by its name and answers as a host that
//! can't would: nothing, or an error saying so.

use fenix_extension_api as real;
use real::{
    Check, Command, CursorImage, CursorInfo, Decoration, DecorationRule, EditorTheme, FolderEntry, GitSummary, Here, HomeSection, HttpRequest, ImageInfo, Item, KeyScheme, LaunchConfig, LayerLevel, Level, ModelInfo,
    Notification, PickerItem, PlatformInfo, Problem, Rgb, SurfaceMark, SurfaceOptions, Task, TaskResult, TextBuffer, TextEdit, TlsOptions,
};

use crate::fake::{line, Fake};

fn with<R>(f: impl FnOnce(&mut crate::fake::State) -> R) -> R {
    Fake::with(f)
}

fn cant<T>(what: &str) -> Result<T, String> {
    Err(format!("the test kit's host doesn't {what}"))
}

fn opt(cmd: Option<&Command>) -> String {
    cmd.map(line).unwrap_or_default()
}

pub fn log(level: Level, message: &str) {
    with(|s| s.log(level, message))
}
pub fn notify(level: Level, message: &str) {
    with(|s| s.notify(level, message))
}
pub fn run_command(id: &str) {
    with(|s| s.other(format!("command {id}")))
}
pub fn open_file(path: &str, line: Option<u32>) {
    with(|s| s.other(match line {
        Some(l) => format!("open {path}:{l}"),
        None => format!("open {path}"),
    }))
}
pub fn show_picker(title: &str, items: &[PickerItem]) -> u64 {
    with(|s| s.pick(title, items, false))
}
pub fn show_picker_many(title: &str, items: &[PickerItem]) -> u64 {
    with(|s| s.pick(title, items, true))
}
pub fn prompt(label: &str, initial: &str) -> u64 {
    with(|s| s.prompt(label, initial, false))
}
pub fn compose(title: &str, verb: &str, initial: &str, _allow_empty: bool) -> u64 {
    with(|s| s.compose(title, verb, initial))
}
pub fn editor_here() -> Option<Here> {
    with(|s| s.here.clone())
}
pub fn home_set(section: Option<&HomeSection>) {
    with(|s| s.other(format!("home {}", if section.is_some() { "set" } else { "-" })))
}
pub fn page_open(kind: &str, title: &str) -> u64 {
    with(|s| s.open_page(kind, title, None))
}
pub fn page_open_beside(kind: &str, title: &str, width: f32) -> u64 {
    with(|s| s.open_page(kind, title, Some(width)))
}
pub fn page_refresh(page: u64) {
    with(|s| s.refresh(page))
}
pub fn page_close(page: u64) {
    with(|s| {
        s.other(format!("page-close {page}"));
        if let Some(p) = s.pages.get_mut(&page) {
            p.open = false;
        }
    })
}
pub fn open_text(title: &str, text: &str, language: Option<&str>) {
    with(|s| s.open_text(title, text, language, None))
}
pub fn open_url(url: &str) -> Result<(), String> {
    with(|s| s.open_url(url))
}
pub fn clipboard_set(text: &str) {
    with(|s| s.copy(text))
}
pub fn clipboard_set_image(png: &[u8]) -> Result<(), String> {
    with(|s| {
        s.other(format!("copy-image {} bytes", png.len()));
        s.clipboard_image = Some(png.to_vec());
        Ok(())
    })
}
pub fn clipboard_image() -> Option<Vec<u8>> {
    with(|s| s.clipboard_image.clone())
}
pub fn buffer_text(path: &str) -> Option<String> {
    with(|s| s.buffers.get(path).cloned())
}
pub fn buffer_edit(path: &str, edits: &[TextEdit]) -> Result<(), String> {
    with(|s| s.edit(path, edits))
}
pub fn buffer_insert(text: &str) -> Result<(), String> {
    with(|s| {
        s.other(format!("insert {text}"));
        Ok(())
    })
}
pub fn buffer_save(path: &str) -> Result<(), String> {
    with(|s| {
        s.other(format!("save {path}"));
        Ok(())
    })
}
pub fn status_set(text: Option<&str>) {
    with(|s| s.status(text))
}
pub fn setting(key: &str) -> Option<String> {
    with(|s| s.setting(key))
}
pub fn theme_colors() -> EditorTheme {
    with(|s| s.theme.clone())
}
pub fn setting_set(key: &str, value: Option<&str>) -> Result<(), String> {
    with(|s| s.set_setting(key, value))
}
pub fn secret(key: &str) -> Option<String> {
    with(|s| s.secrets.get(key).cloned())
}
pub fn find_program(name: &str, _hints: &[String]) -> Option<String> {
    with(|s| s.find_program(name))
}
pub fn new_project(args: &str) {
    with(|s| s.other(format!("new-project {args}")))
}
pub fn spawn(cmd: &Command) -> Result<u64, String> {
    with(|s| s.spawn(cmd, false))
}
pub fn write_stdin(process: u64, data: &[u8]) -> Result<(), String> {
    with(|s| s.write_stdin(process, data))
}
pub fn kill(process: u64) {
    with(|s| s.kill(process))
}
pub fn run_in_terminal(cmd: &Command, title: &str, below: bool) -> Result<u64, String> {
    with(|s| {
        let id = s.id();
        s.other(format!("terminal {id} {title}{}: {}", if below { " (below)" } else { "" }, line(cmd)));
        Ok(id)
    })
}
pub fn terminal_stop(terminal: u64) {
    with(|s| s.other(format!("terminal-stop {terminal}")))
}
pub fn terminal_restart(terminal: u64, cmd: Option<&Command>, _title: Option<&str>) -> Result<(), String> {
    with(|s| {
        s.other(format!("terminal-restart {terminal} {}", opt(cmd)));
        Ok(())
    })
}
pub fn run_task(name: &str, cmd: &Command) -> Result<u64, String> {
    with(|s| s.run_task(name, cmd, false))
}
pub fn utc_offset(_at: i64) -> i32 {
    with(|s| s.utc_offset)
}
pub fn set_timer(millis: u32, repeat: bool) -> u64 {
    with(|s| s.set_timer(millis, repeat))
}
pub fn cancel_timer(timer: u64) {
    with(|s| s.cancel_timer(timer))
}
pub fn call_service(name: &str, payload: &str) -> u64 {
    with(|s| s.call_service(name, payload))
}
pub fn service_answer(call: u64, answer: Result<&str, &str>) {
    with(|s| s.answer(call, answer))
}
pub fn image_load(bytes: &[u8]) -> Result<ImageInfo, String> {
    with(|s| {
        s.other(format!("image-load {} bytes", bytes.len()));
        cant("decode pictures")
    })
}
pub fn image_file(path: &str) -> Result<ImageInfo, String> {
    with(|s| {
        s.other(format!("image-file {path}"));
        cant("decode pictures")
    })
}
pub fn image_free(image: u32) {
    with(|s| s.other(format!("image-free {image}")))
}
pub fn lsp_send(server: u64, message: &str) {
    with(|s| s.other(format!("lsp {server} {message}")))
}
pub fn language_server_set(languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
    with(|s| s.language_server(languages, cmd))
}
pub fn language_server_restart() {
    with(|s| s.other("server-restart"))
}
pub fn doctor_set(checks: &[Check]) {
    with(|s| s.doctor(checks))
}
pub fn tasks_set(tasks: &[Task]) -> Result<(), String> {
    with(|s| s.set_tasks(tasks))
}
pub fn sheet_load(path: &str, _frame_width: u32, _frame_height: u32) -> Result<u32, String> {
    with(|s| {
        s.other(format!("sheet {path}"));
        cant("draw layers")
    })
}
pub fn model_load(path: &str) -> Result<ModelInfo, String> {
    with(|s| {
        s.other(format!("model {path}"));
        cant("draw layers")
    })
}
pub fn layer_open(_level: LayerLevel) -> Result<u64, String> {
    with(|s| {
        s.other("layer-open");
        cant("draw layers")
    })
}
pub fn layer_set(layer: u64, items: &[Item]) {
    with(|s| s.other(format!("layer-set {layer} {}", items.len())))
}
pub fn layer_close(layer: u64) {
    with(|s| s.other(format!("layer-close {layer}")))
}
pub fn socket_connect(host: &str, port: u16) -> Result<u64, String> {
    with(|s| {
        s.other(format!("socket {host}:{port}"));
        cant("open sockets")
    })
}
pub fn socket_write(socket: u64, data: &[u8]) -> Result<(), String> {
    with(|s| {
        s.other(format!("socket-write {socket} {} bytes", data.len()));
        cant("open sockets")
    })
}
pub fn udp_bind(port: u16) -> Result<u64, String> {
    with(|s| {
        s.other(format!("udp-bind {port}"));
        cant("open sockets")
    })
}
pub fn udp_send(socket: u64, host: &str, port: u16, data: &[u8]) -> Result<(), String> {
    with(|s| {
        s.other(format!("udp-send {socket} {host}:{port} {} bytes", data.len()));
        cant("open sockets")
    })
}
pub fn socket_close(socket: u64) {
    with(|s| s.other(format!("socket-close {socket}")))
}
pub fn surface_open(kind: &str, title: &str, _options: SurfaceOptions) -> Result<u64, String> {
    with(|s| Ok(s.open_page(kind, title, None)))
}
pub fn surface_resize(surface: u64, width: u32, height: u32) -> Result<(), String> {
    with(|s| {
        s.other(format!("surface-resize {surface} {width}x{height}"));
        Ok(())
    })
}
pub fn surface_write(surface: u64, x: u32, y: u32, width: u32, height: u32, _bgra: &[u8]) -> Result<(), String> {
    with(|s| {
        s.other(format!("surface-write {surface} {x},{y} {width}x{height}"));
        Ok(())
    })
}
pub fn surface_fill(surface: u64, x: u32, y: u32, width: u32, height: u32, _color: Rgb) -> Result<(), String> {
    with(|s| {
        s.other(format!("surface-fill {surface} {x},{y} {width}x{height}"));
        Ok(())
    })
}
pub fn surface_copy(surface: u64, x: u32, y: u32, width: u32, height: u32, to_x: u32, to_y: u32) -> Result<(), String> {
    with(|s| {
        s.other(format!("surface-copy {surface} {x},{y} {width}x{height} -> {to_x},{to_y}"));
        Ok(())
    })
}
pub fn surface_present(surface: u64) {
    with(|s| s.other(format!("surface-present {surface}")))
}
pub fn surface_cursor(surface: u64, _cursor: Option<&CursorImage>) {
    with(|s| s.other(format!("surface-cursor {surface}")))
}
pub fn surface_marks(surface: u64, marks: &[SurfaceMark]) {
    with(|s| s.other(format!("surface-marks {surface} {}", marks.len())))
}
pub fn page_retitle(page: u64, title: &str) {
    with(|s| {
        s.other(format!("page-retitle {page} {title}"));
        if let Some(p) = s.pages.get_mut(&page) {
            p.title = title.into();
        }
    })
}
pub fn spawn_raw(cmd: &Command) -> Result<u64, String> {
    with(|s| s.spawn(cmd, true))
}
pub fn clipboard_text() -> Option<String> {
    with(|s| s.clipboard.clone())
}
pub fn file_read(path: &str) -> Result<Vec<u8>, String> {
    with(|s| s.files.get(path).cloned().ok_or_else(|| format!("{path}: not found")))
}
pub fn explorer_selected() -> Option<String> {
    with(|s| s.explorer.clone())
}
pub fn git_refresh() {
    with(|s| s.other("git-refresh"))
}
pub fn page_focus(page: u64) {
    with(|s| s.other(format!("page-focus {page}")))
}
pub fn editor_cursor() -> Option<CursorInfo> {
    with(|s| s.cursor)
}
pub fn scratch_open(title: &str, text: &str, _language: Option<&str>) -> u64 {
    with(|s| s.scratch_open(title, text, None))
}
pub fn scratch_text(scratch: u64) -> Option<String> {
    with(|s| s.scratches.get(&scratch).map(|(_, t)| t.clone()))
}
pub fn scratch_set(scratch: u64, text: &str) -> Result<(), String> {
    with(|s| {
        s.other(format!("scratch-set {scratch}"));
        match s.scratches.get_mut(&scratch) {
            Some(slot) => {
                slot.1 = text.into();
                Ok(())
            }
            None => Err(format!("no scratch buffer {scratch}")),
        }
    })
}
pub fn scratch_close(scratch: u64) {
    with(|s| {
        s.other(format!("scratch-close {scratch}"));
        s.scratches.remove(&scratch);
    })
}
pub fn folder_list(path: &str) -> Result<Vec<FolderEntry>, String> {
    with(|s| s.folders.get(path).cloned().ok_or_else(|| format!("{path}: not found")))
}
pub fn play_sound(path: &str) -> Result<(), String> {
    with(|s| {
        s.other(format!("sound {path}"));
        Ok(())
    })
}
pub fn decorations_set(buffer: &TextBuffer, decorations: &[Decoration]) -> Result<(), String> {
    with(|s| s.decorate(buffer, decorations))
}
pub fn buffer_undo(path: &str) -> Result<(), String> {
    with(|s| {
        s.other(format!("undo {path}"));
        Ok(())
    })
}
pub fn buffer_redo(path: &str) -> Result<(), String> {
    with(|s| {
        s.other(format!("redo {path}"));
        Ok(())
    })
}
pub fn theme_palette() -> Vec<Rgb> {
    with(|s| s.palette.clone())
}
pub fn debug_adapter_set(languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
    with(|s| s.debug_adapter(languages, cmd, None))
}
pub fn formatter_set(languages: &[String], cmd: Option<&Command>, keep_output: bool) -> Result<(), String> {
    with(|s| s.formatter(languages, cmd, keep_output))
}
pub fn diagnostics_set(path: &str, problems: &[Problem]) -> Result<(), String> {
    with(|s| s.diagnostics(path, problems))
}
pub fn close_stdin(process: u64) {
    with(|s| s.close_stdin(process))
}
pub fn scratch_open_beside(title: &str, text: &str, _language: Option<&str>, width: f32) -> u64 {
    with(|s| s.scratch_open(title, text, Some(width)))
}
pub fn open_text_beside(title: &str, text: &str, language: Option<&str>, width: f32) {
    with(|s| s.open_text(title, text, language, Some(width)))
}
pub fn http_download(req: &HttpRequest, path: &str, sha256: Option<&str>) -> Result<u64, String> {
    with(|s| s.download(req, path, sha256))
}
pub fn set_executable(path: &str) -> Result<(), String> {
    with(|s| s.set_executable(path))
}
pub fn http(req: &HttpRequest) -> Result<u64, String> {
    with(|s| s.http(req))
}
pub fn http_cancel(request: u64) {
    with(|s| s.other(format!("http-cancel {request}")))
}
pub fn socket_connect_tls(host: &str, port: u16, _tls: &TlsOptions) -> Result<u64, String> {
    with(|s| {
        s.other(format!("socket-tls {host}:{port}"));
        cant("open sockets")
    })
}
pub fn socket_start_tls(socket: u64, _tls: &TlsOptions) -> Result<(), String> {
    with(|s| {
        s.other(format!("start-tls {socket}"));
        cant("open sockets")
    })
}
pub fn ipc_connect(name: &str) -> Result<u64, String> {
    with(|s| {
        s.other(format!("ipc {name}"));
        cant("connect to local programs")
    })
}
pub fn debug_adapter_connect(languages: &[String], cmd: Option<&Command>, port: u16) -> Result<(), String> {
    with(|s| s.debug_adapter(languages, cmd, Some(port)))
}
pub fn decoration_rules_set(rules: &[DecorationRule]) -> Result<(), String> {
    with(|s| {
        s.other(format!("decoration-rules {}", rules.len()));
        Ok(())
    })
}
pub fn page_file(page: u64, path: Option<&str>) -> Result<(), String> {
    with(|s| {
        s.other(format!("page-file {page} {}", path.unwrap_or("-")));
        Ok(())
    })
}
pub fn platform() -> PlatformInfo {
    with(|s| s.platform.clone())
}
pub fn buffer_language_set(buffer: &TextBuffer, language: Option<&str>) -> Result<(), String> {
    with(|s| {
        s.other(format!("language {} {}", crate::fake::buffer_key(buffer), language.unwrap_or("-")));
        Ok(())
    })
}
pub fn run_task_background(name: &str, cmd: &Command) -> Result<u64, String> {
    with(|s| s.run_task(name, cmd, true))
}
pub fn task_cancel(task: u64) {
    with(|s| s.task_cancel(task))
}
pub fn task_show(task: u64) -> Result<(), String> {
    with(|s| s.task_show(task))
}
pub fn language_server_add(name: &str, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
    with(|s| s.add_language_server(name, languages, cmd))
}
pub fn process_id() -> u32 {
    4242
}
pub fn language_server_settings(name: &str, settings: Option<&str>) -> Result<(), String> {
    with(|s| s.server_settings(name, settings))
}
pub fn launches_set(launches: &[LaunchConfig]) -> Result<(), String> {
    with(|s| s.set_launches(launches))
}
pub fn launch_variable_set(name: &str, choices: &[PickerItem]) -> Result<(), String> {
    with(|s| s.launch_variable(name, choices))
}
pub fn secret_set(key: &str, value: Option<&str>) -> Result<(), String> {
    with(|s| s.set_secret(key, value))
}
pub fn prompt_secret(label: &str) -> u64 {
    with(|s| s.prompt(label, "", true))
}
pub fn tcp_listen(port: u16) -> Result<(u64, u16), String> {
    with(|s| {
        s.other(format!("listen {port}"));
        cant("listen on ports")
    })
}
pub fn env_var(name: &str) -> Option<String> {
    with(|s| s.env.get(name).cloned())
}
pub fn agent_provide(id: &str, cmd: Option<&Command>) -> Result<(), String> {
    with(|s| {
        s.other(format!("agent {id}: {}", opt(cmd)));
        Ok(())
    })
}

// -- What API 1.1 to 1.5 added ---------------------------------------------

/// 1.1: the open project's repository, as the test set it
/// ([`Fake::git`](crate::Fake::git)).
pub fn git_state() -> Option<GitSummary> {
    with(|s| s.git.clone())
}
/// 1.1: the task that ended last ([`Fake::last_task`](crate::Fake::last_task),
/// or one [`Fake::on_task`](crate::Fake::on_task) ended).
pub fn last_task() -> Option<TaskResult> {
    with(|s| s.last_task.clone())
}
/// 1.2: how far a service call has got.
pub fn service_progress(call: u64, message: &str, fraction: Option<f32>) {
    with(|s| s.progress(call, message, fraction))
}
/// 1.3: a notification. `Notification::show` is the API's method, which
/// calls Fenix's: in a test, call this with it.
pub fn notify_with(notification: &Notification) {
    with(|s| s.notify_with(notification))
}
/// 1.3: its toast with this tag taken away.
pub fn notify_dismiss(tag: &str) {
    with(|s| s.notify_dismiss(tag))
}
/// 1.4: a compose pane the agent can write; answered as `compose` is
/// ([`Fake::on_prompt`](crate::Fake::on_prompt)).
pub fn compose_with_ask(title: &str, verb: &str, initial: &str, _allow_empty: bool, ask: &str, over_initial: bool) -> u64 {
    with(|s| s.compose_asking(title, verb, initial, Some((ask, over_initial))))
}
/// 1.5: how you edit ([`Fake::key_scheme`](crate::Fake::key_scheme)).
pub fn editor_keys() -> KeyScheme {
    with(|s| s.key_scheme)
}
/// 1.5: the keys that run `command` in the scheme in use, as the test set
/// them ([`Fake::key`](crate::Fake::key)); none for a command it said
/// nothing of, or one bound only in `SPC m`
/// ([`Fake::menu_key`](crate::Fake::menu_key)).
pub fn key_for(command: &str) -> Option<String> {
    with(|s| s.key_for(command))
}

/// Each of these is the API's own signature: a call that builds against
/// one builds against the other.
macro_rules! same {
    ($($name:ident: fn($($t:ty),*) $(-> $r:ty)?;)*) => {
        $(const _: [fn($($t),*) $(-> $r)?; 2] = [$name, real::host::$name];)*
    };
}

same! {
    log: fn(Level, &str);
    notify: fn(Level, &str);
    run_command: fn(&str);
    open_file: fn(&str, Option<u32>);
    show_picker: fn(&str, &[PickerItem]) -> u64;
    show_picker_many: fn(&str, &[PickerItem]) -> u64;
    prompt: fn(&str, &str) -> u64;
    compose: fn(&str, &str, &str, bool) -> u64;
    editor_here: fn() -> Option<Here>;
    home_set: fn(Option<&HomeSection>);
    page_open: fn(&str, &str) -> u64;
    page_open_beside: fn(&str, &str, f32) -> u64;
    page_refresh: fn(u64);
    page_close: fn(u64);
    open_text: fn(&str, &str, Option<&str>);
    open_url: fn(&str) -> Result<(), String>;
    clipboard_set: fn(&str);
    clipboard_set_image: fn(&[u8]) -> Result<(), String>;
    clipboard_image: fn() -> Option<Vec<u8>>;
    buffer_text: fn(&str) -> Option<String>;
    buffer_edit: fn(&str, &[TextEdit]) -> Result<(), String>;
    buffer_insert: fn(&str) -> Result<(), String>;
    buffer_save: fn(&str) -> Result<(), String>;
    status_set: fn(Option<&str>);
    setting: fn(&str) -> Option<String>;
    theme_colors: fn() -> EditorTheme;
    setting_set: fn(&str, Option<&str>) -> Result<(), String>;
    secret: fn(&str) -> Option<String>;
    find_program: fn(&str, &[String]) -> Option<String>;
    new_project: fn(&str);
    spawn: fn(&Command) -> Result<u64, String>;
    write_stdin: fn(u64, &[u8]) -> Result<(), String>;
    kill: fn(u64);
    run_in_terminal: fn(&Command, &str, bool) -> Result<u64, String>;
    terminal_stop: fn(u64);
    terminal_restart: fn(u64, Option<&Command>, Option<&str>) -> Result<(), String>;
    run_task: fn(&str, &Command) -> Result<u64, String>;
    utc_offset: fn(i64) -> i32;
    set_timer: fn(u32, bool) -> u64;
    cancel_timer: fn(u64);
    call_service: fn(&str, &str) -> u64;
    service_answer: fn(u64, Result<&str, &str>);
    image_load: fn(&[u8]) -> Result<ImageInfo, String>;
    image_file: fn(&str) -> Result<ImageInfo, String>;
    image_free: fn(u32);
    lsp_send: fn(u64, &str);
    language_server_set: fn(&[String], Option<&Command>) -> Result<(), String>;
    language_server_restart: fn();
    doctor_set: fn(&[Check]);
    tasks_set: fn(&[Task]) -> Result<(), String>;
    sheet_load: fn(&str, u32, u32) -> Result<u32, String>;
    model_load: fn(&str) -> Result<ModelInfo, String>;
    layer_open: fn(LayerLevel) -> Result<u64, String>;
    layer_set: fn(u64, &[Item]);
    layer_close: fn(u64);
    socket_connect: fn(&str, u16) -> Result<u64, String>;
    socket_write: fn(u64, &[u8]) -> Result<(), String>;
    udp_bind: fn(u16) -> Result<u64, String>;
    udp_send: fn(u64, &str, u16, &[u8]) -> Result<(), String>;
    socket_close: fn(u64);
    surface_open: fn(&str, &str, SurfaceOptions) -> Result<u64, String>;
    surface_resize: fn(u64, u32, u32) -> Result<(), String>;
    surface_write: fn(u64, u32, u32, u32, u32, &[u8]) -> Result<(), String>;
    surface_fill: fn(u64, u32, u32, u32, u32, Rgb) -> Result<(), String>;
    surface_copy: fn(u64, u32, u32, u32, u32, u32, u32) -> Result<(), String>;
    surface_present: fn(u64);
    surface_cursor: fn(u64, Option<&CursorImage>);
    surface_marks: fn(u64, &[SurfaceMark]);
    page_retitle: fn(u64, &str);
    spawn_raw: fn(&Command) -> Result<u64, String>;
    clipboard_text: fn() -> Option<String>;
    file_read: fn(&str) -> Result<Vec<u8>, String>;
    explorer_selected: fn() -> Option<String>;
    git_refresh: fn();
    page_focus: fn(u64);
    editor_cursor: fn() -> Option<CursorInfo>;
    scratch_open: fn(&str, &str, Option<&str>) -> u64;
    scratch_text: fn(u64) -> Option<String>;
    scratch_set: fn(u64, &str) -> Result<(), String>;
    scratch_close: fn(u64);
    folder_list: fn(&str) -> Result<Vec<FolderEntry>, String>;
    play_sound: fn(&str) -> Result<(), String>;
    decorations_set: fn(&TextBuffer, &[Decoration]) -> Result<(), String>;
    buffer_undo: fn(&str) -> Result<(), String>;
    buffer_redo: fn(&str) -> Result<(), String>;
    theme_palette: fn() -> Vec<Rgb>;
    debug_adapter_set: fn(&[String], Option<&Command>) -> Result<(), String>;
    formatter_set: fn(&[String], Option<&Command>, bool) -> Result<(), String>;
    diagnostics_set: fn(&str, &[Problem]) -> Result<(), String>;
    close_stdin: fn(u64);
    scratch_open_beside: fn(&str, &str, Option<&str>, f32) -> u64;
    open_text_beside: fn(&str, &str, Option<&str>, f32);
    http_download: fn(&HttpRequest, &str, Option<&str>) -> Result<u64, String>;
    set_executable: fn(&str) -> Result<(), String>;
    http: fn(&HttpRequest) -> Result<u64, String>;
    http_cancel: fn(u64);
    socket_connect_tls: fn(&str, u16, &TlsOptions) -> Result<u64, String>;
    socket_start_tls: fn(u64, &TlsOptions) -> Result<(), String>;
    ipc_connect: fn(&str) -> Result<u64, String>;
    debug_adapter_connect: fn(&[String], Option<&Command>, u16) -> Result<(), String>;
    decoration_rules_set: fn(&[DecorationRule]) -> Result<(), String>;
    page_file: fn(u64, Option<&str>) -> Result<(), String>;
    platform: fn() -> PlatformInfo;
    buffer_language_set: fn(&TextBuffer, Option<&str>) -> Result<(), String>;
    run_task_background: fn(&str, &Command) -> Result<u64, String>;
    task_cancel: fn(u64);
    task_show: fn(u64) -> Result<(), String>;
    language_server_add: fn(&str, &[String], Option<&Command>) -> Result<(), String>;
    process_id: fn() -> u32;
    language_server_settings: fn(&str, Option<&str>) -> Result<(), String>;
    launches_set: fn(&[LaunchConfig]) -> Result<(), String>;
    launch_variable_set: fn(&str, &[PickerItem]) -> Result<(), String>;
    secret_set: fn(&str, Option<&str>) -> Result<(), String>;
    prompt_secret: fn(&str) -> u64;
    tcp_listen: fn(u16) -> Result<(u64, u16), String>;
    env_var: fn(&str) -> Option<String>;
    agent_provide: fn(&str, Option<&Command>) -> Result<(), String>;
    // 1.1 to 1.5.
    git_state: fn() -> Option<GitSummary>;
    last_task: fn() -> Option<TaskResult>;
    service_progress: fn(u64, &str, Option<f32>);
    notify_with: fn(&Notification);
    notify_dismiss: fn(&str);
    compose_with_ask: fn(&str, &str, &str, bool, &str, bool) -> u64;
    editor_keys: fn() -> KeyScheme;
    key_for: fn(&str) -> Option<String>;
}
