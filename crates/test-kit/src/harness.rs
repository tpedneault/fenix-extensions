//! Driving an extension as Fenix would: its exports called, and what the
//! fake has for it -- a process's lines, a picker's answer -- given to it
//! after each, as events.

use fenix_extension_api::{Changed, Event, Extension, Focus, Key, KeyMods, KeyPress, KeyScheme, MouseAction, MouseButton, MouseEvent, MousePress, NamedKey, Page, SurfaceKey};

use crate::fake::Fake;
use crate::render;

/// The most events given to an extension after one call: past it, an
/// answer is making another without end.
const MOST_EVENTS: usize = 10_000;

/// An extension and the fake host it talks to.
pub struct Harness<E: Extension> {
    pub ext: E,
    pub fake: Fake,
    /// Each call is followed by the events waiting (on by default); off,
    /// [`Harness::settle`] gives them.
    pub settles: bool,
    /// The size pages are laid out at by [`Harness::screen`].
    pub size: (u32, u32),
}

impl<E: Extension> Default for Harness<E> {
    fn default() -> Self {
        Harness::new()
    }
}

impl<E: Extension> Harness<E> {
    /// A fresh fake, then the extension as Fenix makes it (`Default`).
    pub fn new() -> Harness<E> {
        Harness::with_fake(Fake::new())
    }

    /// The extension made (`Default`) once `fake`, set up already, is this
    /// thread's: what it reads as it's made is the fake's.
    pub fn with_fake(fake: Fake) -> Harness<E> {
        fake.install();
        Harness::wrap(fake, E::default())
    }

    /// An extension made some other way (a language pack given the fake
    /// as its kit's host), on `fake`.
    pub fn wrap(fake: Fake, ext: E) -> Harness<E> {
        fake.install();
        Harness { ext, fake, settles: true, size: (80, 24) }
    }

    fn after<R>(&mut self, r: R) -> R {
        if self.settles {
            self.settle();
        }
        r
    }

    /// The events waiting, given to the extension in order, and those
    /// they bring, until there are none.
    pub fn settle(&mut self) {
        self.fake.install();
        for _ in 0..MOST_EVENTS {
            let Some(event) = self.fake.next_event() else { return };
            self.ext.handle(event);
        }
        panic!("more than {MOST_EVENTS} events after one call: an answer keeps asking again");
    }

    pub fn activate(&mut self) -> Result<(), String> {
        let r = self.ext.activate();
        self.after(r)
    }

    /// One of its commands, by id.
    pub fn command(&mut self, id: &str) -> Result<(), String> {
        let r = self.ext.run_command(id);
        self.after(r)
    }

    /// An event, as Fenix sends it.
    pub fn event(&mut self, event: Event) {
        self.ext.handle(event);
        self.after(())
    }

    /// One of its settings changed to `value` (or unset): set in the fake,
    /// then said (`setting-changed`).
    pub fn set_setting(&mut self, key: &str, value: Option<&str>) {
        match value {
            Some(v) => self.fake.state_mut().settings.insert(key.into(), v.into()),
            None => self.fake.state_mut().settings.remove(key),
        };
        self.event(Event::SettingChanged(key.into()))
    }

    /// A key on page `page`.
    pub fn key(&mut self, page: u64, key: Key) {
        self.ext.page_key(page, key);
        self.after(())
    }

    /// Keys on page `page`, typed: each character a key, and `<Enter>`,
    /// `<Esc>`, `<Tab>`, `<S-Tab>`, `<BS>`, `<Del>`, `<Up>`, `<Down>`,
    /// `<Left>`, `<Right>`, `<Home>`, `<End>`, `<lt>` (a `<`) by name.
    pub fn keys(&mut self, page: u64, keys: &str) {
        for key in parse_keys(keys) {
            self.key(page, key);
        }
    }

    /// A key a page that takes more keys gets (`page-key-press`): PageUp,
    /// Ctrl+Z.
    pub fn press(&mut self, page: u64, key: KeyPress) {
        self.ext.page_key_press(page, key);
        self.after(())
    }

    /// Ctrl with `c` on page `page`.
    pub fn ctrl(&mut self, page: u64, c: char) {
        self.press(page, KeyPress { key: SurfaceKey::Char(c), mods: KeyMods { shift: false, ctrl: true, alt: false, meta: false } })
    }

    /// A named key, PageUp or the like, on page `page`.
    pub fn named(&mut self, page: u64, key: NamedKey) {
        self.press(page, KeyPress { key: SurfaceKey::Named(key), mods: KeyMods { shift: false, ctrl: false, alt: false, meta: false } })
    }

    /// Text pasted on page `page`.
    pub fn paste(&mut self, page: u64, text: &str) {
        self.ext.page_paste(page, text);
        self.after(())
    }

    /// The mouse on page `page`.
    pub fn mouse(&mut self, page: u64, event: MouseEvent) {
        self.ext.page_mouse(page, event);
        self.after(())
    }

    /// A left click (`clicks` 2 for a double) at `line` and cell `cell`
    /// of page `page`, its column taken to be the cell's.
    pub fn click(&mut self, page: u64, line: u32, cell: u32, clicks: u8) {
        let mods = KeyMods { shift: false, ctrl: false, alt: false, meta: false };
        self.mouse(page, MouseEvent { line, column: cell, cell, action: MouseAction::Press(MousePress { button: MouseButton::Left, clicks }), mods })
    }

    /// Page `page` closed by you (its tab, `q`): the fake marks it closed,
    /// and the extension is told.
    pub fn close(&mut self, page: u64) {
        if let Some(p) = self.fake.state_mut().pages.get_mut(&page) {
            p.open = false;
        }
        self.ext.page_closed(page);
        self.after(())
    }

    /// A file saved, by path (`file-saved`).
    pub fn saved(&mut self, path: &str) {
        self.ext.file_saved(path);
        self.after(())
    }

    /// The focus moved.
    pub fn focus(&mut self, focus: Focus) {
        self.ext.focus_moved(focus);
        self.after(())
    }

    /// A buffer's text changed: set in the fake first, for a file.
    pub fn changed(&mut self, what: Changed, text: Option<&str>) {
        if let (Changed::File(path), Some(text)) = (&what, text) {
            self.fake.buffer(path, text);
        }
        self.ext.buffer_changed(what);
        self.after(())
    }

    /// One of its services, called by another extension: its answer now,
    /// or `None` when it answers later (`answer`, in the fake's log).
    pub fn serve(&mut self, name: &str, payload: &str) -> Option<Result<String, String>> {
        let call = Fake::with(|s| s.id());
        let r = self.ext.serve(call, name, payload);
        self.after(r)
    }

    /// You switched to `scheme` (Vim keys or standard keys): set in the
    /// fake, so `key_scheme` and `key_for` answer for it, then said
    /// (`key-scheme-changed`, API 1.5).
    pub fn key_scheme(&mut self, scheme: KeyScheme) {
        self.fake.key_scheme(scheme);
        self.ext.key_scheme_changed(scheme);
        self.after(())
    }

    /// Service call `call`, which it said it would answer later, was given
    /// up on (`service-cancelled`, API 1.2).
    pub fn cancelled(&mut self, call: u64) {
        self.ext.service_cancelled(call);
        self.after(())
    }

    /// The answer to its ask `call` so far, all of it
    /// (`agent-ask-progress`, API 1.2).
    pub fn ask_progress(&mut self, call: u64, text: &str) {
        self.ext.agent_ask_progress(call, text);
        self.after(())
    }

    /// Timer `timer` went off: one that doesn't repeat is done with.
    pub fn fire(&mut self, timer: u64) {
        {
            let mut s = self.fake.state_mut();
            if s.timers.get(&timer).is_some_and(|t| !t.repeat) {
                s.timers.remove(&timer);
            }
        }
        self.event(Event::Timer(timer))
    }

    /// Every timer set goes off, once each, in the order they were set.
    pub fn fire_timers(&mut self) {
        for t in self.fake.timers() {
            if self.fake.state().timers.contains_key(&t.id) {
                self.fire(t.id);
            }
        }
    }

    /// Process `process` printed `line` (on its stderr, if `stderr`).
    pub fn said(&mut self, process: u64, line: &str, stderr: bool) {
        self.event(Event::ProcessOutput(fenix_extension_api::ProcessOutput { process, stderr, line: line.into() }))
    }

    /// Process `process` ended with `code`.
    pub fn exited(&mut self, process: u64, code: Option<i32>) {
        self.event(Event::ProcessExit((process, code)))
    }

    /// Page `page` laid out at `width` by `height`.
    pub fn render(&mut self, page: u64, width: u32, height: u32) -> Page {
        self.fake.install();
        self.ext.page(page, width, height)
    }

    /// Page `page` as text ([`render::text`]), laid out at
    /// [`Harness::size`].
    pub fn screen(&mut self, page: u64) -> String {
        let (w, h) = self.size;
        render::text(&self.render(page, w, h))
    }

    /// Page `page` with its marks ([`render::marked`]).
    pub fn marked(&mut self, page: u64) -> String {
        let (w, h) = self.size;
        render::marked(&self.render(page, w, h))
    }

    /// Its newest open page of `kind`; it panics, saying what's open, if
    /// there's none.
    pub fn page(&self, kind: &str) -> u64 {
        self.fake.page(kind).unwrap_or_else(|| panic!("no {kind} page open: {:?}", self.fake.open_pages()))
    }
}

/// Keys as [`Harness::keys`] reads them.
pub fn parse_keys(keys: &str) -> Vec<Key> {
    let mut out = Vec::new();
    let mut rest = keys;
    while let Some(c) = rest.chars().next() {
        if c == '<' {
            if let Some(end) = rest.find('>') {
                let named = match &rest[1..end] {
                    "Enter" | "CR" => Some(Key::Enter),
                    "Esc" => Some(Key::Escape),
                    "Tab" => Some(Key::Tab),
                    "S-Tab" => Some(Key::BackTab),
                    "BS" => Some(Key::Backspace),
                    "Del" => Some(Key::Delete),
                    "Up" => Some(Key::Up),
                    "Down" => Some(Key::Down),
                    "Left" => Some(Key::Left),
                    "Right" => Some(Key::Right),
                    "Home" => Some(Key::Home),
                    "End" => Some(Key::End),
                    "lt" => Some(Key::Char('<')),
                    _ => None,
                };
                if let Some(key) = named {
                    out.push(key);
                    rest = &rest[end + 1..];
                    continue;
                }
            }
        }
        out.push(Key::Char(c));
        rest = &rest[c.len_utf8()..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_typed_and_named() {
        assert_eq!(parse_keys("a<Enter>b<lt><x>"), [Key::Char('a'), Key::Enter, Key::Char('b'), Key::Char('<'), Key::Char('<'), Key::Char('x'), Key::Char('>')]);
        assert_eq!(parse_keys("<S-Tab><BS>é"), [Key::BackTab, Key::Backspace, Key::Char('é')]);
    }
}
