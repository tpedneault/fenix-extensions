//! The `plan` page for now: the project's state in a few lines -- whether
//! kiln is there, the project's name, how many items, each milestone's
//! progress, the findings -- or what's in the way, as kiln said it.
//!
//! A stand-in, kept small on purpose: E1.1's page frame (tabs, a filter,
//! the cursor on an item) replaces it whole. It draws nothing it doesn't
//! know and offers one key, `r`, read again.

use crate::api::{page as pages, Page, Span};
use crate::model::{count, Counts, MIN_VERSION};
use crate::project::State;

/// The page for `state`.
pub fn page(state: State<'_>) -> Page {
    let head = |p: Page, title: &str| p.line([pages::title(title.to_string())]);
    let p = Page::new();
    let p = match state {
        State::Idle | State::Probing => head(p, "Kiln").line([pages::muted("looking for kiln…")]),
        State::NotFound(why) => {
            head(p, "Kiln")
                .line([pages::bad(why.to_string())])
                .line([pages::muted(
                    "Kiln's releases: https://github.com/tpedneault/kiln/releases",
                )])
        }
        State::NotKiln(why) => head(p, "Kiln")
            .line([pages::bad(format!("kiln can't be used: {why}"))])
            .line([pages::muted("set kiln.path to Kiln's kiln")]),
        State::TooOld(v) => head(p, "Kiln").line([pages::bad(format!(
            "kiln {v} is older than {MIN_VERSION}, the oldest this extension reads: update Kiln"
        ))]),
        State::NoProject(line) => head(p, "Kiln").line([pages::warn(line.to_string())]),
        State::Loading => head(p, "Kiln").line([pages::muted("reading the project…")]),
        State::Failed(line) => head(p, "Kiln").line([pages::bad(line.to_string())]),
        State::Loaded {
            loaded,
            reloading,
            error,
        } => {
            let mut p = head(p, loaded.name().unwrap_or("Kiln")).line([pages::muted(format!(
                "{} · kiln {}",
                count(loaded.items.len(), "item"),
                loaded.version
            ))]);
            match &loaded.plan {
                Ok(_) => {
                    for g in loaded.groups() {
                        let (id, title) = g
                            .milestone
                            .as_ref()
                            .map_or(("", "no milestone"), |m| (m.id.as_str(), m.title.as_str()));
                        let progress = format!("{} of {} done", g.done, g.total);
                        let mut spans = Vec::new();
                        if !id.is_empty() {
                            spans.push(pages::accent(format!("{id} ")));
                        }
                        spans.push(pages::text(format!("{title}  ")));
                        spans.push(if g.done == g.total {
                            pages::good(progress)
                        } else {
                            pages::muted(progress)
                        });
                        p = p.line(spans);
                    }
                    if loaded.groups().is_empty() {
                        p = p.line([pages::muted("no open milestones")]);
                    }
                }
                Err(why) => p = p.line([pages::warn(format!("no plan: {why}"))]),
            }
            p = match (loaded.counts(), &loaded.findings) {
                (_, Err(why)) => p.line([pages::warn(format!("not checked: {why}"))]),
                (Some(c), Ok(_)) => p.line([findings(c)]),
                (None, Ok(_)) => p,
            };
            if let Some(e) = error {
                p = p
                    .line([pages::bad(format!("the last read failed: {e}"))])
                    .line([pages::muted("shown: the read before it")]);
            }
            if reloading {
                p = p.line([pages::muted("reading again…")]);
            }
            p
        }
    };
    p.key("r", "read again")
}

fn findings(c: Counts) -> Span {
    match (c.errors, c.warnings) {
        (0, 0) => pages::good("no findings"),
        (0, w) => pages::warn(count(w as usize, "warning")),
        (e, 0) => pages::bad(count(e as usize, "error")),
        (e, w) => pages::bad(format!(
            "{}, {}",
            count(e as usize, "error"),
            count(w as usize, "warning")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{self, tests::fixture, Version};
    use crate::project::Loaded;
    use fenix_test_kit::render;

    fn screen(state: State<'_>) -> String {
        render::text(&page(state))
    }

    fn demo() -> Loaded {
        Loaded::new(
            Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            model::list(&fixture("demo/list.json")).unwrap(),
            model::status(&fixture("demo/status.json")),
            model::check(&fixture("demo/check.json")),
        )
    }

    #[test]
    fn loaded_says_the_name_items_milestones_and_findings() {
        let d = demo();
        assert_eq!(
            screen(State::Loaded { loaded: &d, reloading: false, error: None }),
            "Demo\n16 items · kiln 1.0.0\nMS-001 First release  1 of 5 done\nMS-002 Second release  0 of 2 done\nno milestone  0 of 1 done\n1 error, 2 warnings\n\nr read again\n"
        );
    }

    #[test]
    fn a_failed_reload_is_said_under_what_was_loaded() {
        let d = demo();
        let s = screen(State::Loaded {
            loaded: &d,
            reloading: true,
            error: Some("kiln: bad front matter"),
        });
        assert!(s.contains("1 error, 2 warnings\nthe last read failed: kiln: bad front matter\nshown: the read before it\nreading again…\n"), "{s}");
    }

    #[test]
    fn whats_in_the_way_is_said() {
        assert_eq!(screen(State::NotFound("kiln isn't on the PATH: install Kiln, or set kiln.path to where kiln is")), "Kiln\nkiln isn't on the PATH: install Kiln, or set kiln.path to where kiln is\nKiln's releases: https://github.com/tpedneault/kiln/releases\n\nr read again\n");
        assert!(screen(State::TooOld(Version {
            major: 0,
            minor: 9,
            patch: 0
        }))
        .contains("kiln 0.9.0 is older than 1.0.0"));
        assert!(screen(State::NoProject("kiln: no kiln.toml in <dir>"))
            .contains("\nkiln: no kiln.toml in <dir>\n"));
        assert!(screen(State::Loading).contains("reading the project…"));
        assert!(screen(State::Failed("kiln: oops")).contains("\nkiln: oops\n"));
    }

    #[test]
    fn kilns_own_project_has_no_open_milestones() {
        let own = Loaded::new(
            Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            model::list(&fixture("own/list.json")).unwrap(),
            model::status(&fixture("own/status.json")),
            model::check(&fixture("own/check.json")),
        );
        let s = screen(State::Loaded {
            loaded: &own,
            reloading: false,
            error: None,
        });
        assert!(
            s.starts_with("Kiln\n") && s.contains("no open milestones\nno findings\n"),
            "{s}"
        );
    }
}
