//! Pets in Fenix: little 3D pets that live on the modeline, as a
//! WebAssembly extension. `brain` decides what they do; here that's drawn
//! as a layer (`layer::set`) of posed models and effects, which Fenix
//! animates, and what Fenix says is given back.

pub mod acts;
pub mod brain;
pub mod care;
pub mod fx;
pub mod home;
pub mod save;
pub mod season;
pub mod world;

use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use fenix_extension_api::{export, host, layer, page, Event, Extension, Key, LayerLevel, Page, PickerItem, Style};

use acts::KINDS;
use brain::{Home, State};
use home::Action;
use world::Screen;

/// Who the pets are, kept between runs.
const KEPT: &str = "/data/pets.json";
/// How often they decide, in ms.
const TICK: u32 = 200;

/// What a picker or a prompt was for.
enum Asked {
    Kind,
    Name(String),
    Rehome,
    Dress(u32),
    Rename(u32),
}

#[derive(Default)]
struct Pets {
    state: Option<State>,
    started: Option<Instant>,
    layer: Option<u64>,
    models: HashMap<String, u32>,
    ball: Option<u32>,
    fx: Option<u32>,
    asked: HashMap<u64, Asked>,
    /// The home page, if it's open, and the pet it's on.
    home: Option<u64>,
    focus: usize,
    kept_at: u64,
}

/// Days since 1970, today.
fn today() -> u32 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| (d.as_secs() / 86_400) as u32).unwrap_or(0)
}

impl Pets {
    fn now(&self) -> u64 {
        self.started.map(|s| s.elapsed().as_millis() as u64).unwrap_or(0)
    }

    fn state(&mut self) -> &mut State {
        self.state.get_or_insert_with(|| State::new(Vec::new(), 1, today()))
    }

    /// The layer drawn again from where the pets are now.
    fn draw(&mut self) {
        let now = self.now();
        let (Some(layer), Some(st)) = (self.layer, self.state.as_ref()) else { return };
        layer::set(layer, &st.items(now, &self.models, self.ball, self.fx));
    }

    fn keep(&mut self) {
        let text = save::save(&self.state().pets);
        if let Err(e) = std::fs::write(KEPT, text) {
            fenix_extension_api::warn!("{KEPT}: {e}");
        }
        self.state().dirty = false;
        self.kept_at = self.now();
    }

    /// What they said, told; and the home page drawn again.
    fn tell(&mut self) {
        for said in std::mem::take(&mut self.state().said) {
            fenix_extension_api::notify(&said);
        }
        if let Some(home) = self.home {
            page::refresh(home);
        }
    }

    /// The settings, read again.
    fn settings(&mut self) {
        let setting = |key: &str| fenix_extension_api::setting(key);
        let size = match setting("pets.size").as_deref() {
            Some("small") => 1.6,
            Some("large") => 3.0,
            _ => 2.2,
        };
        let home = if setting("pets.place").as_deref() == Some("panes") { Home::Panes } else { Home::Modeline };
        let most = setting("pets.most").and_then(|m| m.trim().parse().ok()).unwrap_or(5usize).clamp(1, 12);
        let style = if setting("pets.style").as_deref() == Some("smooth") { Style::Smooth } else { Style::Pixel };
        let off = |key: &str| setting(key).as_deref() == Some("false");
        let (seasons, visits) = (!off("pets.seasons"), !off("pets.visits"));
        let st = self.state();
        st.size = size;
        st.home = home;
        st.most = most;
        st.style = style;
        st.seasons = seasons;
        st.visits = visits;
    }

    fn rehome(&mut self) {
        let items: Vec<PickerItem> = self.state().pets.iter().map(|p| PickerItem::new(p.name.clone(), p.uid.to_string()).detail(p.kind.clone())).collect();
        if items.is_empty() {
            fenix_extension_api::notify("No pets to rehome");
            return;
        }
        let picker = fenix_extension_api::pick("Rehome a pet", &items);
        self.asked.insert(picker, Asked::Rehome);
    }

    /// What pet `uid` can wear, to pick from: picking one puts it on or
    /// takes it off.
    fn dress(&mut self, uid: u32) {
        let Some(pet) = self.state().pets.iter().find(|p| p.uid == uid) else { return };
        let unlocked = pet.care.unlocked();
        if unlocked.is_empty() {
            fenix_extension_api::notify(&format!("{} has nothing to wear yet -- a bow comes at level 2", pet.name));
            return;
        }
        let items: Vec<PickerItem> = care::UNLOCKS
            .iter()
            .filter(|(_, part, _)| unlocked.contains(part))
            .map(|(_, part, what)| PickerItem::new(*what, *part).detail(if pet.wearing.iter().any(|w| w == part) { "wearing -- take it off" } else { "put it on" }))
            .collect();
        let picker = fenix_extension_api::pick(&format!("Dress {}", pet.name), &items);
        self.asked.insert(picker, Asked::Dress(uid));
    }

    fn adopt(&mut self) {
        let items: Vec<PickerItem> = KINDS.iter().map(|(kind, about, _)| PickerItem::new(*about, *kind)).collect();
        let picker = fenix_extension_api::pick("Adopt a pet", &items);
        self.asked.insert(picker, Asked::Kind);
    }
}

impl Extension for Pets {
    fn activate(&mut self) -> Result<(), String> {
        self.started = Some(Instant::now());
        let mut clips = HashMap::new();
        for (kind, _, _) in KINDS {
            match layer::model(&format!("models/{kind}.glb")) {
                Ok(info) => {
                    self.models.insert(kind.to_string(), info.id);
                    clips.insert(kind.to_string(), info.clips.into_iter().collect());
                }
                Err(e) => fenix_extension_api::warn!("models/{kind}.glb: {e}"),
            }
        }
        self.ball = layer::model("models/ball.glb").map(|info| info.id).ok();
        self.fx = layer::sheet("sprites/fx.png", (16, 16)).ok();
        self.layer = Some(layer::open(LayerLevel::AboveText)?);
        let day = today();
        let seed = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        // The first time, Ember comes to stay.
        let (pets, first) = match std::fs::read_to_string(KEPT) {
            Ok(text) => (save::load(&text, day), false),
            Err(_) => (Vec::new(), true),
        };
        let mut st = State::new(pets, seed, day);
        st.clips = clips;
        self.state = Some(st);
        self.settings();
        if first {
            self.state().adopt("ember", "Ember", 0);
        }
        self.keep();
        host::set_timer(TICK, true);
        self.draw();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let now = self.now();
        match id {
            "pets.adopt" => self.adopt(),
            "pets.throw_ball" => {
                if !self.state().throw_ball(now) {
                    fenix_extension_api::notify("No pets to throw it for -- SPC P a adopts one");
                }
            }
            "pets.hide" => {
                let st = self.state();
                st.hidden = !st.hidden;
                let said = if st.hidden { "The pets are napping out of sight -- SPC P h brings them back" } else { "The pets are back" };
                fenix_extension_api::notify(said);
            }
            "pets.rehome" => self.rehome(),
            "pets.home" => {
                self.home = Some(page::open("home", "Pets"));
            }
            "pets.dress" => {
                let items: Vec<PickerItem> = self.state().pets.iter().map(|p| PickerItem::new(p.name.clone(), p.uid.to_string()).detail(format!("level {}", p.care.level()))).collect();
                match items.len() {
                    0 => fenix_extension_api::notify("No pets yet -- SPC P a adopts one"),
                    1 => {
                        let uid = self.state().pets[0].uid;
                        self.dress(uid);
                    }
                    _ => {
                        let picker = fenix_extension_api::pick("Dress which pet?", &items);
                        self.asked.insert(picker, Asked::Dress(0));
                    }
                }
            }
            other => return Err(format!("no command {other}")),
        }
        self.draw();
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        let now = self.now();
        match event {
            Event::Timer(_) | Event::Arrived(_) => {
                self.state().tick(now);
                if self.state().dirty && now.saturating_sub(self.kept_at) > 30_000 {
                    self.keep();
                }
            }
            Event::Layout(layout) => self.state().layout(Screen::from_layout(&layout), now),
            Event::LayerClicked((_, item)) => self.state().petted(item, now),
            Event::Saved(_) => self.state().saved(now),
            Event::Diagnostics((errors, _)) => self.state().diagnostics(errors, now),
            Event::TaskEnded(ok) => self.state().task_ended(ok, now),
            Event::Typing => self.state().typing(now),
            Event::SettingChanged(_) => self.settings(),
            Event::PageShown((page, false)) if Some(page) == self.home => {}
            Event::Picked((id, Some(value))) => match self.asked.remove(&id) {
                Some(Asked::Kind) => {
                    let initial = KINDS.iter().find(|(k, _, _)| *k == value).map_or("Pet", |(_, _, name)| name);
                    let prompt = fenix_extension_api::prompt("Its name", initial);
                    self.asked.insert(prompt, Asked::Name(value));
                }
                Some(Asked::Rehome) => {
                    if let Some(pet) = value.parse().ok().and_then(|uid| self.state().rehome(uid)) {
                        fenix_extension_api::notify(&format!("{} has a new home -- thank you for having them", pet.name));
                        self.focus = 0;
                        self.keep();
                    }
                }
                Some(Asked::Dress(0)) => {
                    if let Ok(uid) = value.parse() {
                        self.dress(uid);
                    }
                }
                Some(Asked::Dress(uid)) => {
                    if let Some(pet) = self.state().pets.iter_mut().find(|p| p.uid == uid) {
                        match pet.wearing.iter().position(|w| *w == value) {
                            Some(at) => {
                                pet.wearing.remove(at);
                            }
                            None => {
                                // One hat at a time.
                                if care::HATS.contains(&value.as_str()) {
                                    pet.wearing.retain(|w| !care::HATS.contains(&w.as_str()));
                                }
                                pet.wearing.push(value);
                            }
                        }
                        self.keep();
                    }
                }
                _ => {}
            },
            Event::Prompted((id, Some(text))) => match self.asked.remove(&id) {
                Some(Asked::Name(kind)) => {
                    let name = if text.trim().is_empty() { kind.clone() } else { text.trim().to_string() };
                    let most = self.state().most;
                    if self.state().adopt(&kind, &name, now) {
                        fenix_extension_api::notify(&format!("{name} has come to stay"));
                        self.keep();
                    } else {
                        fenix_extension_api::notify(&format!("There's room for {most} pets (pets.most) -- SPC P r rehomes one"));
                    }
                }
                Some(Asked::Rename(uid)) if !text.trim().is_empty() => {
                    if let Some(pet) = self.state().pets.iter_mut().find(|p| p.uid == uid) {
                        pet.name = text.trim().to_string();
                    }
                    self.keep();
                }
                _ => {}
            },
            Event::Picked((id, None)) | Event::Prompted((id, None)) => {
                self.asked.remove(&id);
            }
            _ => return,
        }
        self.tell();
        self.draw();
    }

    fn page(&mut self, _page: u64, width: u32, _height: u32) -> Page {
        let focus = self.focus;
        home::render(self.state(), focus, width)
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let mut focus = self.focus;
        let action = home::key(self.state(), &mut focus, key);
        self.focus = focus;
        let now = self.now();
        match action {
            Action::Nothing => return,
            Action::Moved => {}
            Action::Pet(uid) => self.state().petted(uid, now),
            Action::Dress(uid) => self.dress(uid),
            Action::Rename(uid) => {
                let name = self.state().pets.iter().find(|p| p.uid == uid).map(|p| p.name.clone()).unwrap_or_default();
                let prompt = fenix_extension_api::prompt("A new name", &name);
                self.asked.insert(prompt, Asked::Rename(uid));
            }
            Action::Rehome(uid) => {
                let name = self.state().pets.iter().find(|p| p.uid == uid).map(|p| p.name.clone()).unwrap_or_default();
                let items = [PickerItem::new(format!("Rehome {name}"), uid.to_string()).detail("they'll be well looked after")];
                let picker = fenix_extension_api::pick("Rehome?", &items);
                self.asked.insert(picker, Asked::Rehome);
            }
            Action::Adopt => self.adopt(),
        }
        page::refresh(page);
        self.draw();
    }

    fn page_state(&mut self, _page: u64) -> Option<String> {
        Some(String::new())
    }

    fn page_restore(&mut self, page: u64, kind: &str, _state: &str) -> bool {
        if kind == "home" {
            self.home = Some(page);
        }
        kind == "home"
    }
}

export!(Pets);
