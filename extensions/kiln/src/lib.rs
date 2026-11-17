//! Kiln in Fenix: a Kiln project's plan on pages, edits to its items,
//! exports and read tools for coding agents -- all by running the `kiln`
//! program (`--format json` to read; `set`, `link` and `new` to change).
//!
//! What it deliberately doesn't do: it never writes an item file itself,
//! compiles none of Kiln in, reaches no network and holds no Confluence
//! or Jira credential. Kiln's own checks and edits are the only ones;
//! this extension shows what kiln said and asks kiln to act.
//!
//! This file meets Fenix. For now it holds the commands and the one page
//! kind, `plan`, which says only that the project is being read; the job
//! queue that runs kiln, its typed answers and the pages themselves are
//! added beside it, each in a module of its own, so this file stays the
//! thin part that talks to the host.

#[cfg(not(test))]
pub(crate) use fenix_extension_api as api;
#[cfg(test)]
pub(crate) use fenix_test_kit::api;

use api::{page as pages, Extension, Page};

/// The page kind the plan opens as (the manifest's context names it).
const PLAN: &str = "plan";

#[derive(Default)]
struct Kiln;

impl Extension for Kiln {
    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "kiln.open" => {
                pages::open(PLAN, "Kiln");
                Ok(())
            }
            "kiln.reload" => {
                api::notify("Kiln: nothing to read yet");
                Ok(())
            }
            _ => Err(format!("no command {id}")),
        }
    }

    fn page(&mut self, _page: u64, _width: u32, _height: u32) -> Page {
        Page::new()
            .line([pages::title("Kiln")])
            .line([pages::muted("reading the project…")])
    }
}

api::export!(Kiln);

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_test_kit::{Fake, Harness};

    #[test]
    fn open_shows_the_plan_page() {
        let fake = Fake::new();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        assert_eq!(h.screen(page), "Kiln\nreading the project…\n");
    }

    #[test]
    fn reload_says_there_is_nothing_yet() {
        let fake = Fake::new();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.reload").unwrap();
        assert_eq!(fake.notifications(), ["Kiln: nothing to read yet"]);
    }

    #[test]
    fn an_unknown_command_is_an_error() {
        let mut h: Harness<Kiln> = Harness::new();
        assert_eq!(
            h.command("kiln.nope"),
            Err("no command kiln.nope".to_string())
        );
    }
}
