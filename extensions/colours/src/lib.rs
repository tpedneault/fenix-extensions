//! Colour literals made visible and editable: a swatch beside every one in
//! the file you're in (and those you've been in), kept up to date as you
//! type; a picker for the one under the cursor that writes it back the
//! way it was written; conversions between hex, RGB, HSL and OKLCH; a
//! WCAG contrast checker; and the project's palette, grouped by hue and
//! by near-duplicates, each colour opened to where it's used.
//!
//! Literals: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()`,
//! `hsl()`, `hsla()` and `oklch()`, old syntax and new; CSS's named
//! colours in stylesheets and markup; and a Fenix theme's
//! `{ color = "#rrggbb", alpha = 0.12 }`.

mod colour;
mod contrast;
mod draw;
mod edit;
mod ignore;
mod literal;
mod names;
mod palette;
mod palette_page;
mod picker;
mod scan;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use fenix_extension_api::{self as api, buffer, decorations, export, host, page, Changed, Decoration, Event, Extension, Focus, Key, KeyPress, MouseEvent, Page, PickerItem, TextEdit};
use fenix_ui_kit as kit;

use colour::Colour;
use contrast::{Checker, Side};
use literal::{Format, Target};
use palette::{Palette, Project, Walk};
use palette_page::PalettePage;
use picker::{Picker, Place};

/// The files swatched lately, whose changes it keeps up with.
const KEEP: usize = 16;
/// How long a palette tick reads for, and how often one comes.
const TICK: Duration = Duration::from_millis(40);
const TICK_EVERY: u32 = 15;

enum View {
    Picker(Box<Picker>),
    Contrast(Box<Checker>),
    Palette(Box<PalettePage>),
}

/// What a picker it opened is for.
enum Ask {
    /// The conversion of these literals in this file.
    Convert(String, Vec<(Place, literal::Literal)>),
}

/// The project being read for the palette.
struct Reading {
    walk: Walk,
    timer: u64,
    started: Instant,
}

#[derive(Default)]
pub struct Colours {
    views: HashMap<u64, View>,
    asks: HashMap<u64, Ask>,
    /// The file you're in.
    focused: Option<String>,
    /// The files swatched lately, the latest last.
    swatched: Vec<String>,
    /// The editor's background, for see-through swatches.
    background: Option<Colour>,
    palette: Palette,
    reading: Option<Reading>,
    /// How the last reading went: files, time.
    read: Option<(usize, usize, Duration)>,
}

fn error(message: &str) {
    host::notify(api::Level::Error, message);
}

fn setting_bool(key: &str, default: bool) -> bool {
    api::setting(key).map_or(default, |v| v.trim() == "true")
}

/// CSS's legacy syntax (`rgb(1, 2, 3)`) for what's converted from hex or
/// a name.
fn legacy() -> bool {
    api::setting("colours.syntax").is_some_and(|v| v.trim() == "legacy")
}

fn rgb_colour((r, g, b): (u8, u8, u8)) -> Colour {
    Colour::from_u8(r, g, b)
}

/// The file you're in, its text, and the cursor (line, column); and the
/// selection, when there's one.
type Where = (String, String, (usize, usize), Option<((usize, usize), (usize, usize))>);

fn where_you_are() -> Result<Where, String> {
    let here = api::here().ok_or("not in a file")?;
    let path = here.path.ok_or("not in a file: a colour is changed in a file's buffer")?;
    let cursor = api::cursor().ok_or("no cursor")?;
    let text = buffer::text(&path).ok_or("its buffer can't be read")?;
    let at = (cursor.at.0 as usize, cursor.at.1 as usize);
    let sel = cursor.selection.map(|((a, b), (c, d))| ((a as usize, b as usize), (c as usize, d as usize)));
    Ok((path, text, at, sel))
}

/// Changes to one file's literals, as one change you can undo.
fn apply(path: &str, changes: &[edit::Change]) -> Result<(), String> {
    let edits: Vec<TextEdit> = changes.iter().map(|c| TextEdit::new((c.line as u32, c.start as u32), (c.line as u32, c.end as u32), c.text.clone())).collect();
    buffer::edit(path, &edits)
}

impl Colours {
    fn background(&mut self) -> Colour {
        *self.background.get_or_insert_with(|| rgb_colour(api::theme().background))
    }

    fn swatches_on(&self) -> bool {
        setting_bool("colours.swatches", true)
    }

    /// The swatches of the buffer on `path`, as its text is now.
    fn swatch(&mut self, path: &str) {
        if !self.swatches_on() {
            return;
        }
        let Some(text) = buffer::text(path) else { return };
        let limits = scan::Limits { most: api::setting("colours.most").and_then(|v| v.trim().parse().ok()).unwrap_or(2000).min(10_000), ..scan::Limits::default() };
        // Around the cursor, when it's in this file and the file is long.
        let near = match (api::here(), api::cursor()) {
            (Some(h), Some(c)) if h.path.as_deref() == Some(path) => c.at.0 as usize,
            _ => 0,
        };
        let found = scan::literals(&text, literal::Scan::for_path(path), near, limits);
        let back = self.background();
        let decs: Vec<Decoration> = found.iter().map(|f| Decoration::swatch((f.line as u32, f.literal.start as u32), scan::swatch(&f.literal.colour, &back))).collect();
        if let Err(e) = decorations::set(&decorations::file(path), &decs) {
            api::warn!("swatches for {path}: {e}");
            return;
        }
        self.swatched.retain(|p| p != path);
        self.swatched.push(path.to_string());
        if self.swatched.len() > KEEP {
            self.swatched.remove(0);
        }
    }

    /// Every file's swatches again (the theme changed), or taken away.
    fn swatch_all(&mut self) {
        let paths = self.swatched.clone();
        for path in paths {
            if self.swatches_on() {
                self.swatch(&path);
            } else {
                let _ = decorations::set(&decorations::file(&path), &[]);
            }
        }
        if !self.swatches_on() {
            self.swatched.clear();
        }
    }

    fn open_picker(&mut self) -> Result<(), String> {
        let (path, text, at, _) = where_you_are()?;
        let picker = match edit::at_cursor(&text, &path, at.0, at.1) {
            Some((place, lit)) => Picker::new(lit.colour, lit.format, Some(place)),
            // Nothing there: one to type in at the cursor, as hex.
            None => {
                let place = Place { path, line: at.0, start: at.1, end: at.1, text: String::new() };
                Picker::new(rgb_colour(api::theme().accent), Format::Hex { digits: 6, upper: false }, Some(place))
            }
        };
        let id = page::open("picker", "Colour");
        self.views.insert(id, View::Picker(Box::new(picker)));
        Ok(())
    }

    /// The picker's colour written into its literal, and the picker
    /// closed.
    fn write_picker(&mut self, id: u64) {
        let Some(View::Picker(p)) = self.views.get(&id) else { return };
        let Some(place) = p.place.clone() else { return };
        let Some(text) = buffer::text(&place.path) else { return error("its file isn't open any more") };
        let Some((start, end)) = edit::locate(&text, &place) else { return error("the colour changed meanwhile: not written") };
        let change = edit::Change { line: place.line, start, end, text: p.written() };
        match apply(&place.path, &[change]) {
            Ok(()) => {
                self.views.remove(&id);
                page::close(id);
            }
            Err(e) => error(&e),
        }
    }

    /// The literal at the cursor (or each one selected) written as
    /// `target`; none given, a picker asks which.
    fn convert(&mut self, target: Option<Target>) -> Result<(), String> {
        let (path, text, at, sel) = where_you_are()?;
        let found = match sel {
            Some((from, to)) => edit::in_range(&text, &path, from, to),
            None => edit::at_cursor(&text, &path, at.0, at.1).into_iter().collect(),
        };
        let Some((_, first)) = found.first() else { return Err("no colour here".into()) };
        match target {
            Some(t) => self.convert_to(&path, &found, t),
            None => {
                let items: Vec<PickerItem> = edit::offers(first, legacy())
                    .into_iter()
                    .map(|(t, text, mine)| {
                        let label = if found.len() > 1 { format!("{} ({} colours)", t.name(), found.len()) } else { t.name().to_string() };
                        PickerItem::new(label, t.name()).detail(if mine { format!("{text}  (as it is)") } else { text })
                    })
                    .collect();
                let ask = api::pick("Convert to", &items);
                self.asks.insert(ask, Ask::Convert(path, found));
                Ok(())
            }
        }
    }

    fn convert_to(&mut self, path: &str, found: &[(Place, literal::Literal)], target: Target) -> Result<(), String> {
        let text = buffer::text(path).ok_or("its file isn't open any more")?;
        // Each where it is now; a literal changed meanwhile is left be.
        let mut changes = Vec::new();
        for c in edit::convert(found, target, legacy()) {
            let place = found.iter().map(|(p, _)| p).find(|p| p.line == c.line && p.start == c.start).ok_or("lost")?;
            if let Some((start, end)) = edit::locate(&text, place) {
                changes.push(edit::Change { start, end, ..c });
            }
        }
        if changes.is_empty() {
            api::notify(&format!("already {}", target.name()));
            return Ok(());
        }
        apply(path, &changes)
    }

    fn open_contrast(&mut self) -> Result<(), String> {
        let under = self.background();
        let checker = match where_you_are() {
            Ok((path, text, at, _)) => {
                let (_, found) = edit::on_line(&text, at.0, literal::Scan::for_path(&path));
                match edit::at_cursor(&text, &path, at.0, at.1) {
                    Some((place, lit)) => {
                        // The other colour: another on the line, else the editor's.
                        let other = found.iter().filter(|l| l.start != lit.start).min_by_key(|l| l.start.abs_diff(lit.start));
                        let back = match other {
                            Some(o) => {
                                let line: Vec<char> = text.lines().nth(at.0).unwrap_or("").chars().collect();
                                let p = Place { path: path.clone(), line: at.0, start: o.start, end: o.end, text: line[o.start..o.end].iter().collect() };
                                Side::new(o.colour, Some((p, o.format.clone())))
                            }
                            None => Side::new(under, None),
                        };
                        Checker::new(Side::new(lit.colour, Some((place, lit.format))), back, under)
                    }
                    None => Checker::new(Side::new(rgb_colour(api::theme().text), None), Side::new(under, None), under),
                }
            }
            Err(_) => Checker::new(Side::new(rgb_colour(api::theme().text), None), Side::new(under, None), under),
        };
        let id = page::open("contrast", "Contrast");
        self.views.insert(id, View::Contrast(Box::new(checker)));
        Ok(())
    }

    fn write_contrast(&mut self, id: u64) {
        let Some(View::Contrast(k)) = self.views.get(&id) else { return };
        let mut by_file: HashMap<String, Vec<edit::Change>> = HashMap::new();
        for side in [&k.text, &k.back] {
            let Some((place, format)) = &side.from else { continue };
            if !side.changed() {
                continue;
            }
            let Some(text) = buffer::text(&place.path) else { continue };
            match edit::locate(&text, place) {
                Some((start, end)) => by_file.entry(place.path.clone()).or_default().push(edit::Change { line: place.line, start, end, text: format.write(&side.colour) }),
                None => return error("the colour changed meanwhile: not written"),
            }
        }
        for (path, changes) in by_file {
            if let Err(e) = apply(&path, &changes) {
                return error(&e);
            }
        }
        // Written: what it says now is what's there.
        if let Some(View::Contrast(k)) = self.views.get_mut(&id) {
            for side in [&mut k.text, &mut k.back] {
                if let Some((place, format)) = &mut side.from {
                    let new = format.write(&side.colour);
                    place.end = place.start + new.chars().count();
                    place.text = new;
                }
            }
        }
        api::notify("written");
    }

    fn open_palette(&mut self) {
        let id = page::open("palette", "Palette");
        self.views.insert(id, View::Palette(Box::new(PalettePage::new())));
        if self.reading.is_none() && self.read.is_none() {
            self.start_reading();
        }
    }

    fn start_reading(&mut self) {
        if let Some(r) = self.reading.take() {
            host::cancel_timer(r.timer);
        }
        self.palette = Palette::new();
        self.read = None;
        let timer = host::set_timer(TICK_EVERY, true);
        self.reading = Some(Reading { walk: Walk::new(), timer, started: Instant::now() });
        self.tick();
    }

    /// A little more of the project read.
    fn tick(&mut self) {
        let Some(r) = self.reading.as_mut() else { return };
        let until = Instant::now() + TICK;
        while Instant::now() < until {
            if !r.walk.step(&Project, &mut self.palette) {
                break;
            }
        }
        if r.walk.done() {
            host::cancel_timer(r.timer);
            self.read = Some((r.walk.read, r.walk.skipped, r.started.elapsed()));
            self.reading = None;
        }
        self.refresh_palettes();
    }

    fn refresh_palettes(&self) {
        for (id, v) in &self.views {
            if matches!(v, View::Palette(_)) {
                page::refresh(*id);
            }
        }
    }

    fn status(&self) -> String {
        let colours = self.palette.entries.len();
        match (&self.reading, self.read) {
            (Some(r), _) => format!("reading… {} files, {colours} colours", r.walk.read),
            (None, Some((0, _, _))) if !std::path::Path::new("/project").exists() => "no project open".into(),
            (None, Some((files, skipped, took))) => {
                let skipped = if skipped > 0 { format!(", {skipped} passed over") } else { String::new() };
                format!("{colours} colours, {} uses in {files} files{skipped} ({:.1} s)", self.palette.uses, took.as_secs_f64())
            }
            (None, None) => String::new(),
        }
    }

    fn render(&mut self, id: u64, w: u32, h: u32) -> Option<kit::Canvas> {
        let (w, h) = (w as usize, h as usize);
        let status = self.status();
        Some(match self.views.get(&id)? {
            View::Picker(p) => p.render(w, h),
            View::Contrast(k) => k.render(w, h),
            View::Palette(pp) => pp.render(&self.palette, &status, w, h),
        })
    }

    fn palette_act(&mut self, act: palette_page::Act) {
        match act {
            palette_page::Act::Open(path, line) => host::open_file(&format!("/project/{path}"), Some(line)),
            palette_page::Act::Copy(hex) => {
                api::copy(&hex);
                api::notify(&format!("copied {hex}"));
            }
            palette_page::Act::Rescan => self.start_reading(),
            palette_page::Act::None | palette_page::Act::Redraw => {}
        }
    }

    fn picker_act(&mut self, id: u64, act: picker::Act) {
        if act == picker::Act::Write {
            self.write_picker(id);
        }
    }

    fn contrast_act(&mut self, id: u64, act: contrast::Act) {
        if act == contrast::Act::Write {
            self.write_contrast(id);
        }
    }
}

impl Extension for Colours {
    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "colours.pick" => self.open_picker(),
            "colours.convert" => self.convert(None),
            "colours.to_hex" => self.convert(Some(Target::Hex)),
            "colours.to_rgb" => self.convert(Some(Target::Rgb)),
            "colours.to_hsl" => self.convert(Some(Target::Hsl)),
            "colours.to_oklch" => self.convert(Some(Target::Oklch)),
            "colours.contrast" => self.open_contrast(),
            "colours.palette" => {
                self.open_palette();
                Ok(())
            }
            "colours.toggle_swatches" => {
                let on = !self.swatches_on();
                api::set_setting("colours.swatches", Some(if on { "true" } else { "false" }))?;
                if on {
                    if let Some(path) = self.focused.clone() {
                        self.swatch(&path);
                    }
                } else {
                    self.swatch_all();
                }
                api::notify(if on { "swatches on" } else { "swatches off" });
                Ok(())
            }
            _ => Err(format!("no command {id}")),
        }
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Timer(t) if self.reading.as_ref().is_some_and(|r| r.timer == t) => self.tick(),
            Event::Picked((ask, choice)) => {
                let Some(Ask::Convert(path, found)) = self.asks.remove(&ask) else { return };
                if let Some(target) = choice.as_deref().and_then(Target::from_name) {
                    if let Err(e) = self.convert_to(&path, &found, target) {
                        error(&e);
                    }
                }
            }
            Event::ThemeChanged => {
                self.background = None;
                self.swatch_all();
                if let Some(path) = self.focused.clone() {
                    self.swatch(&path);
                }
            }
            Event::SettingChanged(key) if key.starts_with("colours.") => {
                self.swatch_all();
                if let Some(path) = self.focused.clone() {
                    self.swatch(&path);
                }
            }
            _ => {}
        }
    }

    fn focus_moved(&mut self, focus: Focus) {
        if let Focus::File(Some(path)) = focus {
            self.focused = Some(path.clone());
            self.swatch(&path);
        } else if !matches!(focus, Focus::Page(_)) {
            self.focused = None;
        }
    }

    fn buffer_changed(&mut self, what: Changed) {
        if let Changed::File(path) = what {
            if self.focused.as_deref() == Some(path.as_str()) || self.swatched.contains(&path) {
                self.swatch(&path);
            }
        }
    }

    fn page(&mut self, id: u64, width: u32, height: u32) -> Page {
        self.render(id, width, height).map(|c| c.to_page()).unwrap_or_default()
    }

    fn page_key(&mut self, id: u64, key: Key) {
        let key = kit::Key::from(key);
        match self.views.get_mut(&id) {
            Some(View::Picker(p)) => {
                let act = p.key(key);
                self.picker_act(id, act);
            }
            Some(View::Contrast(k)) => {
                let act = k.key(key);
                self.contrast_act(id, act);
            }
            Some(View::Palette(pp)) => {
                let act = pp.key(key, &self.palette);
                self.palette_act(act);
            }
            None => {}
        }
    }

    fn page_key_press(&mut self, id: u64, press: KeyPress) {
        let Some(press) = kit::KeyPress::from_api(&press) else { return };
        match self.views.get_mut(&id) {
            Some(View::Picker(p)) => {
                let act = p.press(press);
                self.picker_act(id, act);
            }
            Some(View::Contrast(k)) => {
                let act = k.press(press);
                self.contrast_act(id, act);
            }
            Some(View::Palette(pp)) if !press.mods.chord() => {
                let act = pp.key(press.key, &self.palette);
                self.palette_act(act);
            }
            _ => {}
        }
    }

    fn page_mouse(&mut self, id: u64, event: MouseEvent) {
        let m = kit::Mouse::from(&event);
        match self.views.get_mut(&id) {
            Some(View::Picker(p)) => {
                p.mouse(&m);
            }
            Some(View::Contrast(k)) => {
                k.mouse(&m);
            }
            Some(View::Palette(pp)) => {
                let act = pp.mouse(&m, &self.palette);
                self.palette_act(act);
            }
            None => {}
        }
    }

    fn page_paste(&mut self, id: u64, text: &str) {
        match self.views.get_mut(&id) {
            Some(View::Picker(p)) => {
                p.paste(text);
            }
            Some(View::Contrast(k)) => {
                k.paste(text);
            }
            _ => {}
        }
    }

    fn page_state(&mut self, id: u64) -> Option<String> {
        match self.views.get(&id)? {
            // Two colours, as hex; where they came from isn't kept.
            View::Contrast(k) => {
                let hex = Format::Hex { digits: 8, upper: false };
                Some(format!("{} {}", hex.write(&k.text.colour), hex.write(&k.back.colour)))
            }
            View::Palette(_) => Some(String::new()),
            // A literal to write into may not be there after a restart.
            View::Picker(_) => None,
        }
    }

    fn page_restore(&mut self, id: u64, kind: &str, state: &str) -> bool {
        match kind {
            "contrast" => {
                let mut parts = state.split_whitespace().filter_map(picker::parse_any);
                let (Some(fg), Some(bg)) = (parts.next(), parts.next()) else { return false };
                let under = self.background();
                self.views.insert(id, View::Contrast(Box::new(Checker::new(Side::new(fg, None), Side::new(bg, None), under))));
                true
            }
            "palette" => {
                self.views.insert(id, View::Palette(Box::new(PalettePage::new())));
                if self.reading.is_none() && self.read.is_none() {
                    self.start_reading();
                }
                true
            }
            _ => false,
        }
    }

    fn page_closed(&mut self, id: u64) {
        self.views.remove(&id);
        if !self.views.values().any(|v| matches!(v, View::Palette(_))) {
            if let Some(r) = self.reading.take() {
                host::cancel_timer(r.timer);
            }
        }
    }
}

export!(Colours);
