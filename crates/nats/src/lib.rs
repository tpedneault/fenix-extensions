//! NATS's client protocol, sans I/O: what a client reads from a server and
//! what it writes, with nothing of sockets, threads or clocks in it.
//!
//! - [`Parser`] takes bytes as they come, in chunks cut anywhere, and gives
//!   back each operation whole -- `INFO`, `MSG`, `HMSG`, `PING`, `PONG`,
//!   `+OK`, `-ERR` -- borrowing its subject, headers and payload from the
//!   chunk (or, for one cut across chunks, from the little it kept). It
//!   allocates nothing for a message.
//! - [`encode`] writes `CONNECT`, `SUB`, `UNSUB`, `PUB`, `HPUB`, `PING` and
//!   `PONG`; [`Connect`] is the `CONNECT` options, with every way to sign
//!   in ([`auth`]: user and password, a token, a JWT from a `.creds` file,
//!   an NKey signing the server's nonce).
//! - [`headers`] reads and writes the `NATS/1.0` header block.
//! - [`subject`] checks subjects and matches wildcards (`*`, `>`).
//! - [`inbox`] makes inbox subjects and keeps requests waiting for replies.
//! - [`jetstream`] and [`kv`] are JetStream's API: its subjects, requests and
//!   answers, for streams, consumers and key-value buckets.

pub mod auth;
pub mod b64;
pub mod encode;
pub mod headers;
pub mod inbox;
mod info;
pub mod jetstream;
pub mod kv;
mod parser;
pub mod subject;

pub use info::{Connect, ServerInfo};
pub use parser::{Msg, Op, ParseError, Parser};
