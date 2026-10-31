//! The PDF reader, as a WebAssembly extension: PDFs read in a pane, with
//! an outline, search, selection, links and marks. `fenix-pdfium`, a small
//! program that comes with Fenix, renders the pages (`protocol`); the
//! `reader` keeps the documents and decides; here what it asks for is done
//! through Fenix, and what Fenix says is given to it.

pub mod font;
pub mod keys;
pub mod layout;
pub mod protocol;
pub mod reader;
pub mod sidebar;
pub mod text;

use fenix_extension_api::{export, host, page, surface, Command, Event, Extension, HomeSection, Key, Level, Page, PickerItem, SurfaceInput, SurfaceMark, SurfaceOptions, SurfaceScale};

use keys::Press;
use reader::{OpenAt, Reader, Rgb};

/// Its own folder: where each PDF was left, and what was read lately.
const DATA: &str = "/data";

/// Fenix, as the reader asks things of it.
struct Fenix;

impl reader::Host for Fenix {
    fn find_companion(&mut self) -> Option<String> {
        fenix_extension_api::find_program(reader::COMPANION, &[])
    }
    fn spawn_companion(&mut self, program: &str) -> Result<u64, String> {
        Command::new(program).spawn_raw()
    }
    fn write_companion(&mut self, process: u64, bytes: &[u8]) -> Result<(), String> {
        host::write_stdin(process, bytes)
    }
    fn kill(&mut self, process: u64) {
        host::kill(process)
    }
    fn surface_open(&mut self, title: &str) -> Result<u64, String> {
        surface::open("pdf", title, &SurfaceOptions::new().scale(SurfaceScale::Fit))
    }
    fn surface_resize(&mut self, s: u64, width: u32, height: u32) {
        if let Err(e) = surface::resize(s, width, height) {
            fenix_extension_api::warn!("surface-resize: {e}");
        }
    }
    fn surface_write(&mut self, s: u64, x: u32, y: u32, width: u32, height: u32, bgra: &[u8]) {
        let _ = surface::write(s, x, y, width, height, bgra);
    }
    fn surface_fill(&mut self, s: u64, x: u32, y: u32, width: u32, height: u32, rgb: Rgb) {
        let _ = surface::fill(s, x, y, width, height, rgb);
    }
    fn surface_copy(&mut self, s: u64, x: u32, y: u32, width: u32, height: u32, to_x: u32, to_y: u32) {
        let _ = surface::copy(s, x, y, width, height, to_x, to_y);
    }
    fn surface_present(&mut self, s: u64) {
        surface::present(s)
    }
    fn surface_marks(&mut self, s: u64, marks: &[SurfaceMark]) {
        surface::marks(s, marks)
    }
    fn retitle(&mut self, p: u64, title: &str) {
        surface::retitle(p, title)
    }
    fn page_open(&mut self, kind: &str, title: &str, beside: Option<f32>) -> u64 {
        match beside {
            Some(width) => page::open_beside(kind, title, width),
            None => page::open(kind, title),
        }
    }
    fn page_refresh(&mut self, p: u64) {
        page::refresh(p)
    }
    fn page_close(&mut self, p: u64) {
        page::close(p)
    }
    fn say(&mut self, level: Level, message: &str) {
        host::notify(level, message)
    }
    fn status(&mut self, text: Option<&str>) {
        fenix_extension_api::status(text)
    }
    fn prompt(&mut self, label: &str, initial: &str) -> u64 {
        fenix_extension_api::prompt(label, initial)
    }
    fn pick(&mut self, title: &str, items: &[(String, String, String)]) -> u64 {
        let items: Vec<PickerItem> = items.iter().map(|(label, detail, value)| PickerItem::new(label.clone(), value.clone()).detail(detail.clone())).collect();
        fenix_extension_api::pick(title, &items)
    }
    fn copy(&mut self, text: &str) {
        fenix_extension_api::copy(text)
    }
    fn open_url(&mut self, url: &str) -> Result<(), String> {
        fenix_extension_api::open_url(url)
    }
    fn open_file(&mut self, path: &str) {
        host::open_file(path, None)
    }
    fn setting(&mut self, key: &str) -> Option<String> {
        fenix_extension_api::setting(key)
    }
    fn set_setting(&mut self, key: &str, value: Option<&str>) -> Result<(), String> {
        fenix_extension_api::set_setting(key, value)
    }
    fn theme(&mut self) -> (Rgb, Rgb) {
        let t = fenix_extension_api::theme();
        (t.background, t.text)
    }
    fn home(&mut self, section: Option<&HomeSection>) {
        fenix_extension_api::home(section)
    }
    fn project_root(&mut self) -> Option<String> {
        fenix_extension_api::here().and_then(|h| h.project).map(|p| p.root)
    }
    fn answer(&mut self, call: u64, result: Result<String, String>) {
        fenix_extension_api::answer(call, result)
    }
    fn timer(&mut self, millis: u32) -> u64 {
        host::set_timer(millis, false)
    }
    fn load(&mut self, name: &str) -> Option<String> {
        std::fs::read_to_string(format!("{DATA}/{name}")).ok()
    }
    fn save(&mut self, name: &str, text: &str) {
        if let Err(e) = std::fs::write(format!("{DATA}/{name}"), text) {
            fenix_extension_api::warn!("{name}: {e}");
        }
    }
}

#[derive(Default)]
struct Pdf {
    reader: Reader,
}

impl Pdf {
    /// What a `SPC r` command acts on: the document read last.
    fn on_target(&mut self, act: impl FnOnce(&mut Reader, &mut Fenix, u64)) -> Result<(), String> {
        match self.reader.target() {
            Some(surface) => {
                act(&mut self.reader, &mut Fenix, surface);
                Ok(())
            }
            None => Err("no PDF open -- SPC r f opens one".into()),
        }
    }
}

impl Extension for Pdf {
    fn activate(&mut self) -> Result<(), String> {
        self.reader.activate(&mut Fenix);
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        use keys::Cmd;
        let cmd = |cmd: Cmd| move |r: &mut Reader, h: &mut Fenix, s: u64| r.run(h, s, cmd);
        match id {
            "pdf.documents" => {
                self.reader.open_shelf(&mut Fenix);
                Ok(())
            }
            "pdf.next_page" => self.on_target(|r, h, s| r.turn_page(h, s, 1)),
            "pdf.prev_page" => self.on_target(|r, h, s| r.turn_page(h, s, -1)),
            "pdf.first_page" => self.on_target(cmd(Cmd::GotoPage(1))),
            "pdf.last_page" => self.on_target(cmd(Cmd::LastPage)),
            "pdf.goto_page" => self.on_target(|r, h, s| r.ask_page(h, s)),
            "pdf.zoom_in" => self.on_target(cmd(Cmd::ZoomIn)),
            "pdf.zoom_out" => self.on_target(cmd(Cmd::ZoomOut)),
            "pdf.fit_page" => self.on_target(cmd(Cmd::FitPage)),
            "pdf.fit_width" => self.on_target(cmd(Cmd::FitWidth)),
            "pdf.toggle_outline" => self.on_target(|r, h, s| r.toggle_outline(h, s)),
            "pdf.search" => self.on_target(cmd(Cmd::Search)),
            "pdf.headings" => self.on_target(|r, h, s| r.pick_heading(h, s)),
            "pdf.copy_link" => self.on_target(|r, h, s| r.copy_link(h, s)),
            "pdf.colors" => {
                self.reader.toggle_colors(&mut Fenix);
                Ok(())
            }
            _ => Err(format!("no command {id}")),
        }
    }

    fn handle(&mut self, event: Event) {
        let host = &mut Fenix;
        match event {
            Event::Opened(path) => self.reader.open(host, &path, OpenAt::Here),
            Event::ProcessBytes((process, bytes)) => self.reader.companion_bytes(host, process, &bytes),
            Event::ProcessOutput(out) if out.stderr => fenix_extension_api::warn!("{}: {}", reader::COMPANION, out.line),
            Event::ProcessExit((process, code)) => self.reader.companion_ended(host, process, code),
            Event::SurfaceResized(size) => self.reader.resized(host, size),
            Event::PageShown((page, shown)) => self.reader.shown(host, page, shown),
            Event::Prompted((id, text)) => self.reader.prompted(host, id, text),
            Event::Picked((id, value)) => self.reader.picked(host, id, value),
            Event::Timer(timer) => self.reader.timer(host, timer),
            Event::SettingChanged(key) => self.reader.setting_changed(host, &key),
            Event::ThemeChanged => self.reader.redraw_all(host),
            Event::HomePicked(path) => self.reader.home_picked(host, &path),
            _ => {}
        }
    }

    fn surface_input(&mut self, surface: u64, input: SurfaceInput) {
        let host = &mut Fenix;
        match input {
            SurfaceInput::Key(key) if key.down => self.reader.key(host, surface, Press::of(&key)),
            SurfaceInput::Pointer(p) => self.reader.pointer(host, surface, p),
            SurfaceInput::Wheel(w) => self.reader.wheel(host, surface, w),
            _ => {}
        }
    }

    fn page(&mut self, page: u64, width: u32, _height: u32) -> Page {
        self.reader.sidebar_page(page, width).unwrap_or_default()
    }

    fn page_key(&mut self, page: u64, key: Key) {
        self.reader.sidebar_key(&mut Fenix, page, key)
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        self.reader.page_state(page)
    }

    fn page_restore(&mut self, _page: u64, kind: &str, state: &str) -> bool {
        self.reader.page_restore(&mut Fenix, kind, state)
    }

    fn serve(&mut self, call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        match name {
            "pdf.open" => Some(self.reader.serve_open(&mut Fenix, payload)),
            "pdf.search" => self.reader.serve_search(&mut Fenix, call, payload),
            _ => Some(Err(format!("no service {name}"))),
        }
    }
}

export!(Pdf);
