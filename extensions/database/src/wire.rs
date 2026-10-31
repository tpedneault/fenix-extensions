//! What the network engines have in common: each is a client that's fed
//! the bytes its socket brings and says what to do next -- bytes to send,
//! TLS to start, that it's ready, a query's outcomes -- so nothing waits
//! in the sandbox, and each is tested against bytes alone.

use crate::result::Outcome;

/// What a client asks of its socket, or says.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Send(Vec<u8>),
    /// TLS from here on (`socket::start_tls`); `tls_ready` when it's up.
    StartTls,
    /// Signed in: queries may go.
    Ready,
    /// The query's outcomes, a statement each.
    Done(Vec<Outcome>),
    /// It can't go on: why. The socket's closed.
    Failed(String),
}

/// A database's wire protocol, as a client of it.
pub trait Client {
    /// What it sends first.
    fn start(&mut self) -> Vec<Action>;
    /// TLS it asked for is up.
    fn tls_ready(&mut self) -> Vec<Action>;
    /// What came on the socket.
    fn receive(&mut self, data: &[u8]) -> Vec<Action>;
    /// Runs `sql` -- statements, as the engine takes several at once.
    fn query(&mut self, sql: &str) -> Vec<Action>;
    /// Runs `sql`, one statement only: the engine refuses more.
    fn query_one(&mut self, sql: &str) -> Vec<Action> {
        self.query(sql)
    }
    /// What it sends to say goodbye.
    fn goodbye(&mut self) -> Vec<u8>;
}

/// What every client is given.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Login {
    pub user: String,
    pub password: String,
    pub database: String,
    /// The most rows a result keeps.
    pub max_rows: usize,
    pub read_only: bool,
}

/// Random bytes, for a nonce: the system's.
pub fn random(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    if getrandom::fill(&mut b).is_err() {
        // No randomness: the clock's, which a nonce can live with.
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        for (i, x) in b.iter_mut().enumerate() {
            *x = (t >> ((i % 16) * 8)) as u8 ^ (i as u8).wrapping_mul(151);
        }
    }
    b
}
