//! What the GitHub Issues and GitLab Issues extensions share that isn't
//! the forge's API (that's `fenix-forge`'s `issues`): their page -- the
//! searches, an issue with its comments, the new-issue form -- laid out
//! with `ui-kit` and answering keys with [`page::Action`]s; the branch
//! started from an issue ([`branch`]); and what their agent tools answer
//! ([`agent`]). None of it calls Fenix, so all of it runs under `cargo
//! test`; each extension is the glue that does.

pub mod agent;
pub mod branch;
pub mod markdown;
pub mod page;
