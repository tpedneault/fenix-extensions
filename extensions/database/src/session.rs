//! A live connection: its socket and the client that speaks over it (or
//! the SQLite file it has open), its queue of jobs -- one runs at a time
//! -- and its schema, once read.

use std::collections::VecDeque;

use crate::conn::{Conn, Engine, Tls};
use crate::lite::Lite;
use crate::result::Outcome;
use crate::schema::Schema;
use crate::wire::{Action, Client, Login};

/// What a session asks of the sockets: Fenix's, or a test's.
pub trait Net {
    fn connect(&mut self, host: &str, port: u16, tls: Option<&Conn>) -> Result<u64, String>;
    fn start_tls(&mut self, socket: u64, conn: &Conn) -> Result<(), String>;
    fn write(&mut self, socket: u64, data: &[u8]) -> Result<(), String>;
    fn close(&mut self, socket: u64);
}

/// What a job is for: where its outcomes go.
#[derive(Debug, Clone, PartialEq)]
pub enum Purpose {
    /// Statements you ran, from a buffer: the browser shows them. A run
    /// may be a job or several (SQL Server's batches), by its id.
    Run(u64),
    /// The catalog's queries.
    Catalog,
    /// A table view's page of rows, or its count.
    Page(u64),
    Count(u64),
    /// A table view's cell changed.
    Update(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub id: u64,
    pub sql: String,
    /// One statement, which the engine holds to (PostgreSQL's extended
    /// query).
    pub single: bool,
    pub purpose: Purpose,
}

/// A job done: what came of it, and how long it took.
#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    pub job: Job,
    pub outcomes: Vec<Outcome>,
    pub millis: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Connecting,
    Ready,
    Failed(String),
    Closed,
}

enum Driver {
    Net { socket: Option<u64>, client: Box<dyn Client>, started: bool },
    Lite(Option<Lite>),
}

pub struct Session {
    pub conn: Conn,
    pub state: State,
    driver: Driver,
    queue: VecDeque<Job>,
    busy: Option<(Job, u64)>,
    finished: Vec<Finished>,
    pub schema: Option<Schema>,
    pub schema_error: Option<String>,
    /// What the server said it is.
    pub server: String,
}

/// Milliseconds since 1970: what a job's time is measured with.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl Session {
    /// A session for `conn`, not yet connected; `password` for its server,
    /// `path` for a SQLite file (where it's opened from).
    pub fn new(conn: Conn, password: &str, max_rows: usize) -> Session {
        let login = Login { user: conn.user.clone(), password: password.to_string(), database: conn.database.clone(), max_rows, read_only: conn.read_only };
        let driver = match conn.engine {
            Engine::Sqlite => Driver::Lite(None),
            Engine::Postgres => Driver::Net { socket: None, client: Box::new(crate::pg::Postgres::new(login, conn.tls)), started: false },
            Engine::Mysql => Driver::Net { socket: None, client: Box::new(crate::mysql::Mysql::new(login, conn.tls)), started: false },
            Engine::Mssql => Driver::Net { socket: None, client: Box::new(crate::tds::Mssql::new(login, conn.tls, &conn.host)), started: false },
        };
        Session { conn, state: State::Connecting, driver, queue: VecDeque::new(), busy: None, finished: Vec::new(), schema: None, schema_error: None, server: String::new() }
    }

    pub fn socket(&self) -> Option<u64> {
        match &self.driver {
            Driver::Net { socket, .. } => *socket,
            Driver::Lite(_) => None,
        }
    }

    pub fn is_lite(&self) -> bool {
        matches!(self.driver, Driver::Lite(_))
    }

    pub fn busy(&self) -> bool {
        self.busy.is_some() || !self.queue.is_empty()
    }

    /// Connects: a socket opened, or the file.
    pub fn connect(&mut self, net: &mut dyn Net) {
        self.state = State::Connecting;
        let conn = self.conn.clone();
        match &mut self.driver {
            Driver::Net { socket, started, .. } => {
                *started = false;
                let strict = conn.tls == Tls::Strict;
                match net.connect(&conn.host, conn.port, strict.then_some(&conn)) {
                    Ok(s) => *socket = Some(s),
                    Err(e) => self.fail(e),
                }
            }
            Driver::Lite(db) => match Lite::open(&conn.database, conn.read_only) {
                Ok(l) => {
                    *db = Some(l);
                    self.state = State::Ready;
                    self.server = "SQLite".into();
                }
                Err(e) => self.fail(e),
            },
        }
    }

    /// Its socket connected (or TLS on it came up), or couldn't.
    pub fn connected(&mut self, net: &mut dyn Net, result: Result<(), String>) {
        if let Err(e) = result {
            return self.fail(format!("{}:{}: {e}", self.conn.host, self.conn.port));
        }
        let Driver::Net { client, started, .. } = &mut self.driver else { return };
        // The first: the protocol starts (TLS from the first byte with
        // SQL Server's strict). After: TLS asked for came up.
        let actions = if *started { client.tls_ready() } else { client.start() };
        *started = true;
        self.apply(net, actions);
    }

    /// What came on its socket.
    pub fn data(&mut self, net: &mut dyn Net, bytes: &[u8]) {
        let Driver::Net { client, .. } = &mut self.driver else { return };
        let actions = client.receive(bytes);
        self.apply(net, actions);
    }

    /// Its socket closed: by the server, or broken.
    pub fn closed(&mut self, why: Option<String>) {
        if let Driver::Net { socket, .. } = &mut self.driver {
            *socket = None;
        }
        if !matches!(self.state, State::Closed | State::Failed(_)) {
            self.fail(why.unwrap_or_else(|| "the server closed the connection".into()));
        }
    }

    /// Says goodbye and lets go.
    pub fn close(&mut self, net: &mut dyn Net) {
        match &mut self.driver {
            Driver::Net { socket, client, .. } => {
                if let Some(s) = socket.take() {
                    let bye = client.goodbye();
                    if !bye.is_empty() {
                        let _ = net.write(s, &bye);
                    }
                    net.close(s);
                }
            }
            Driver::Lite(db) => *db = None,
        }
        self.state = State::Closed;
        self.drain("the connection was closed");
    }

    fn fail(&mut self, why: String) {
        self.state = State::Failed(why.clone());
        self.drain(&why);
    }

    /// Every job waiting given up: `why`.
    fn drain(&mut self, why: &str) {
        let now = now_ms();
        if let Some((job, at)) = self.busy.take() {
            self.finished.push(Finished { job, outcomes: vec![Outcome::Error(why.to_string())], millis: now.saturating_sub(at) });
        }
        for job in self.queue.drain(..) {
            self.finished.push(Finished { job, outcomes: vec![Outcome::Error(why.to_string())], millis: 0 });
        }
    }

    fn apply(&mut self, net: &mut dyn Net, actions: Vec<Action>) {
        for a in actions {
            let socket = self.socket();
            match a {
                Action::Send(bytes) => {
                    if let Some(s) = socket {
                        if let Err(e) = net.write(s, &bytes) {
                            self.fail(e);
                        }
                    }
                }
                Action::StartTls => {
                    if let Some(s) = socket {
                        if let Err(e) = net.start_tls(s, &self.conn) {
                            self.fail(format!("TLS: {e}"));
                        }
                    }
                }
                Action::Ready => {
                    self.state = State::Ready;
                    self.server = match &self.driver {
                        Driver::Net { .. } => self.conn.engine.name().to_string(),
                        Driver::Lite(_) => "SQLite".into(),
                    };
                }
                Action::Done(outcomes) => {
                    if let Some((job, at)) = self.busy.take() {
                        self.finished.push(Finished { job, outcomes, millis: now_ms().saturating_sub(at) });
                    }
                }
                Action::Failed(e) => {
                    if let Some(s) = socket {
                        net.close(s);
                    }
                    if let Driver::Net { socket, .. } = &mut self.driver {
                        *socket = None;
                    }
                    self.fail(e);
                }
            }
        }
        self.pump(net);
    }

    /// A job queued: run now if nothing else is.
    pub fn submit(&mut self, net: &mut dyn Net, job: Job) {
        self.queue.push_back(job);
        self.pump(net);
    }

    /// The next job sent, when it's ready and idle. A SQLite file's jobs
    /// wait for `run_lite`.
    fn pump(&mut self, net: &mut dyn Net) {
        if self.state != State::Ready || self.busy.is_some() || self.is_lite() {
            return;
        }
        let Some(job) = self.queue.pop_front() else { return };
        let Driver::Net { client, .. } = &mut self.driver else { return };
        let actions = if job.single { client.query_one(&job.sql) } else { client.query(&job.sql) };
        self.busy = Some((job, now_ms()));
        self.apply(net, actions);
    }

    /// A SQLite file's jobs, run: each in turn, here.
    pub fn run_lite(&mut self, max_rows: usize) {
        let Driver::Lite(Some(db)) = &mut self.driver else { return };
        while let Some(job) = self.queue.pop_front() {
            let at = now_ms();
            let outcomes = match job.purpose {
                Purpose::Catalog => match db.schema() {
                    Ok(s) => {
                        self.schema = Some(s);
                        self.schema_error = None;
                        Vec::new()
                    }
                    Err(e) => vec![Outcome::Error(e)],
                },
                _ => db.run(&job.sql, max_rows),
            };
            self.finished.push(Finished { job, outcomes, millis: now_ms().saturating_sub(at) });
        }
    }

    /// The jobs done since last asked.
    pub fn take_finished(&mut self) -> Vec<Finished> {
        std::mem::take(&mut self.finished)
    }

    /// What it's doing, for the sidebar and the modeline.
    pub fn status(&self) -> String {
        match &self.state {
            State::Connecting => "connecting…".into(),
            State::Ready if self.busy() => "running…".into(),
            State::Ready => "ready".into(),
            State::Failed(e) => e.clone(),
            State::Closed => "closed".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sockets that keep what's written to them.
    #[derive(Default)]
    struct Wire {
        written: Vec<(u64, Vec<u8>)>,
        tls: Vec<u64>,
        closed: Vec<u64>,
    }

    impl Net for Wire {
        fn connect(&mut self, _host: &str, _port: u16, _tls: Option<&Conn>) -> Result<u64, String> {
            Ok(7)
        }
        fn start_tls(&mut self, socket: u64, _conn: &Conn) -> Result<(), String> {
            self.tls.push(socket);
            Ok(())
        }
        fn write(&mut self, socket: u64, data: &[u8]) -> Result<(), String> {
            self.written.push((socket, data.to_vec()));
            Ok(())
        }
        fn close(&mut self, socket: u64) {
            self.closed.push(socket);
        }
    }

    fn pg(tls: &str) -> Conn {
        Conn::from_row(&["dev", "pg", "localhost", "", "shop", "ada", tls].map(String::from)).unwrap()
    }

    /// A backend message.
    fn msg(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut m = vec![tag];
        m.extend_from_slice(&((body.len() + 4) as u32).to_be_bytes());
        m.extend_from_slice(body);
        m
    }

    #[test]
    fn a_session_signs_in_runs_its_jobs_in_turn_and_fails_them_when_it_breaks() {
        let mut net = Wire::default();
        let mut s = Session::new(pg("tls=prefer"), "pw", 10);
        s.connect(&mut net);
        assert_eq!(s.socket(), Some(7));
        s.connected(&mut net, Ok(()));
        assert_eq!(net.written[0].1, [0, 0, 0, 8, 4, 210, 22, 47], "TLS asked for");
        s.data(&mut net, b"S");
        assert_eq!(net.tls, [7]);
        s.connected(&mut net, Ok(()));
        assert!(net.written[1].1[4..8] == [0, 3, 0, 0], "then the startup message, over TLS");
        let job = |id, purpose| Job { id, sql: "select 1".into(), single: false, purpose };
        s.submit(&mut net, job(1, Purpose::Run(1)));
        s.submit(&mut net, job(2, Purpose::Catalog));
        assert_eq!(net.written.len(), 2, "nothing sent before it's signed in");
        let mut ready = msg(b'R', &0i32.to_be_bytes());
        ready.extend(msg(b'Z', b"I"));
        s.data(&mut net, &ready);
        assert_eq!(s.state, State::Ready);
        assert_eq!(net.written.len(), 3, "the first job, alone");
        assert_eq!(s.status(), "running…");
        let mut done = msg(b'C', b"SELECT 0\0");
        done.extend(msg(b'Z', b"I"));
        s.data(&mut net, &done);
        assert_eq!(net.written.len(), 4, "then the next");
        let f = s.take_finished();
        assert_eq!((f.len(), f[0].job.id), (1, 1));
        s.closed(Some("reset".into()));
        let f = s.take_finished();
        assert_eq!(f[0].job.id, 2);
        assert_eq!(f[0].outcomes, [Outcome::Error("reset".into())]);
        assert_eq!(s.state, State::Failed("reset".into()));
    }

    #[test]
    fn a_refused_connection_and_a_failed_sign_in() {
        let mut net = Wire::default();
        let mut s = Session::new(pg("tls=off"), "pw", 10);
        s.connect(&mut net);
        s.connected(&mut net, Err("connection refused".into()));
        assert_eq!(s.state, State::Failed("localhost:5432: connection refused".into()));
        let mut s = Session::new(pg("tls=off"), "pw", 10);
        s.connect(&mut net);
        s.connected(&mut net, Ok(()));
        s.data(&mut net, &msg(b'E', b"SFATAL\0C28P01\0Mbad password\0\0"));
        assert_eq!(s.state, State::Failed("FATAL 28P01: bad password".into()));
        assert_eq!(net.closed, [7]);
    }
}
