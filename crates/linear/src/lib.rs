//! Linear's GraphQL API, async over a [`Transport`], and the tracker seam
//! (`plans/tracker-seam.md`) served over it: shared by the Linear
//! extension -- which holds your API key and brings its own page -- and
//! the agenda's tests, which run the seam on recorded responses.
//!
//! - `gql`: the one place requests go through; GraphQL errors, rate
//!   limits and a refused key read as the seam's errors.
//! - `queries`: every query, by hand, with variables only.
//! - `model`: Linear's answers, an issue as the seam has it, priorities
//!   0 to 4 and states by type.
//! - `linear`: your issues, one issue, the choices for a new one, the writes.
//! - `seam`: `linear.tracker.*`.
//! - `refs`, `filter`, `branch`, `tools`: what an issue is called, a
//!   typed filter, a branch started from an issue, and what agents are told.

pub mod branch;
pub mod filter;
pub mod gql;
pub mod linear;
pub mod model;
pub mod queries;
pub mod recorded;
pub mod refs;
pub mod seam;
pub mod tools;

pub use gql::{Client, Reply, Sent, Transport, ENDPOINT};
pub use linear::{Linear, View};
pub use model::Detail;
