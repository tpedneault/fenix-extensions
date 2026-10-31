//! VNC in Fenix, as a WebAssembly extension: remote desktops in a pane.
//! `rfb` speaks the protocol, sans I/O; `viewer` keeps the sessions and
//! decides; here what it asks for is done through Fenix, and what Fenix
//! says is given to it.

pub mod des;
pub mod framebuffer;
pub mod keys;
pub mod rfb;
pub mod screenshot;
pub mod viewer;

use fenix_extension_api::{export, host, socket, surface, CursorImage, Event, Extension, Level, PickerItem, SurfaceInput, SurfaceOptions, SurfaceScale};

use viewer::Viewer;

/// Where screenshots go: the `vnc.screenshots` folder, or one in Fenix's
/// data folder (`permissions.folders`).
const SCREENSHOTS: &str = "/folders/screenshots";

/// Fenix, as the viewer asks things of it.
struct Fenix;

impl viewer::Host for Fenix {
    fn connect(&mut self, host: &str, port: u16) -> Result<u64, String> {
        socket::connect(host, port)
    }
    fn write(&mut self, socket: u64, data: &[u8]) -> Result<(), String> {
        socket::write(socket, data)
    }
    fn close(&mut self, socket: u64) {
        socket::close(socket)
    }
    fn timer(&mut self, millis: u32) -> u64 {
        host::set_timer(millis, false)
    }
    fn cancel_timer(&mut self, timer: u64) {
        host::cancel_timer(timer)
    }
    fn surface_open(&mut self, title: &str) -> Result<u64, String> {
        surface::open("vnc", title, &SurfaceOptions::new().capture().scale(SurfaceScale::Stretch))
    }
    fn surface_resize(&mut self, surface: u64, width: u32, height: u32) {
        if let Err(e) = surface::resize(surface, width, height) {
            fenix_extension_api::warn!("surface-resize: {e}");
        }
    }
    fn surface_write(&mut self, surface: u64, x: u32, y: u32, width: u32, height: u32, bgra: &[u8]) {
        let _ = surface::write(surface, x, y, width, height, bgra);
    }
    fn surface_fill(&mut self, surface: u64, x: u32, y: u32, width: u32, height: u32, rgb: (u8, u8, u8)) {
        let _ = surface::fill(surface, x, y, width, height, rgb);
    }
    fn surface_copy(&mut self, surface: u64, x: u32, y: u32, width: u32, height: u32, to_x: u32, to_y: u32) {
        let _ = surface::copy(surface, x, y, width, height, to_x, to_y);
    }
    fn surface_present(&mut self, surface: u64) {
        surface::present(surface)
    }
    fn surface_cursor(&mut self, surface: u64, cursor: Option<CursorImage>) {
        surface::cursor(surface, cursor.as_ref())
    }
    fn retitle(&mut self, page: u64, title: &str) {
        surface::retitle(page, title)
    }
    fn page_close(&mut self, page: u64) {
        fenix_extension_api::page::close(page)
    }
    fn say(&mut self, level: Level, message: &str) {
        host::notify(level, message)
    }
    fn copy(&mut self, text: &str) {
        fenix_extension_api::copy(text)
    }
    fn pasted(&mut self) -> Option<String> {
        fenix_extension_api::pasted_text()
    }
    fn pick(&mut self, title: &str, items: &[(String, String, String)]) -> u64 {
        let items: Vec<PickerItem> = items.iter().map(|(label, detail, value)| PickerItem::new(label.clone(), value.clone()).detail(detail.clone())).collect();
        fenix_extension_api::pick(title, &items)
    }
    fn setting(&mut self, key: &str) -> Option<String> {
        fenix_extension_api::setting(key)
    }
    fn stamp(&mut self) -> String {
        fenix_extension_api::clock::now().format("%Y%m%d-%H%M%S").to_string()
    }
    fn save(&mut self, file: &str, png: &[u8]) -> Result<String, String> {
        std::fs::write(format!("{SCREENSHOTS}/{file}"), png).map_err(|e| e.to_string())?;
        // The folder's setting reads as where it is.
        Ok(match fenix_extension_api::setting("vnc.screenshots").filter(|d| !d.trim().is_empty()) {
            Some(dir) => std::path::Path::new(&dir).join(file).display().to_string(),
            None => file.to_string(),
        })
    }
}

#[derive(Default)]
struct Vnc {
    viewer: Viewer,
}

impl Extension for Vnc {
    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let host = &mut Fenix;
        match id {
            "vnc.open" => {
                self.viewer.open_picker(host);
                Ok(())
            }
            "vnc.close" => self.viewer.close_focused(host),
            "vnc.screenshot" => self.viewer.screenshot_focused(host),
            _ => Err(format!("no command {id}")),
        }
    }

    fn handle(&mut self, event: Event) {
        let host = &mut Fenix;
        match event {
            Event::SocketConnected((socket, result)) => self.viewer.socket_connected(host, socket, result),
            Event::SocketData((socket, data)) => self.viewer.socket_data(host, socket, &data),
            Event::SocketClosed((socket, why)) => self.viewer.socket_closed(host, socket, why),
            Event::Timer(timer) => self.viewer.timer(host, timer),
            Event::Picked((id, value)) => {
                self.viewer.picked(host, id, value.as_deref());
            }
            Event::PageShown((page, shown)) => self.viewer.shown(page, shown),
            Event::SurfaceResized(size) => self.viewer.resized(host, size),
            _ => {}
        }
    }

    fn surface_input(&mut self, surface: u64, input: SurfaceInput) {
        self.viewer.input(&mut Fenix, surface, input)
    }

    fn serve(&mut self, _call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        match name {
            "vnc.connect" => Some(self.viewer.serve_connect(&mut Fenix, payload)),
            _ => Some(Err(format!("no service {name}"))),
        }
    }
}

export!(Vnc);
