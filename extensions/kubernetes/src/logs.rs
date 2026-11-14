//! A log page: a pod's (or a workload's) log as kubectl streams it,
//! followed as it comes, its last 5,000 lines kept. Its keys change how
//! it's read -- one container or all of them, the previous run's, since a
//! while ago -- and each starts the stream again with those options;
//! `s` stops it and starts it again from where it stopped.

use std::collections::VecDeque;

use fenix_ui_kit::text::fit;
use fenix_ui_kit::{Canvas, Key, Role};

use crate::agent::plain;
use crate::kubectl::{self, LogOptions, Target};

/// Lines a log page keeps.
pub const KEPT: usize = 5000;
/// Lines asked for when a log opens.
pub const TAIL: u32 = 200;

/// Whether the stream runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stream {
    Starting,
    Running,
    /// You stopped it.
    Stopped,
    /// It ended by itself: why (the pod went, the run was read whole).
    Ended(String),
}

#[derive(Debug, Clone)]
pub struct LogView {
    pub target: Target,
    pub context: Option<String>,
    /// The pod's containers, for `c`.
    pub containers: Vec<String>,
    pub options: LogOptions,
    pub lines: VecDeque<String>,
    /// Lines that went off the top.
    pub dropped: usize,
    pub stream: Stream,
    /// The first line shown; none: the end, following.
    pub top: Option<usize>,
    pub times: bool,
    pub filter: String,
    /// The last time a line carried, to start again from.
    pub last_time: Option<String>,
}

/// What a key on a log page asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum LogAction {
    Nothing,
    Redraw,
    /// The stream started again with the options as they are now.
    Restart,
    Stop,
    AskSince,
    PickContainer,
    OpenBuffer,
    Filter,
}

/// A line's parts, as kubectl wrote it: `[pod/web-1/nginx] `, the time,
/// the text.
pub fn split(line: &str) -> (Option<&str>, Option<&str>, &str) {
    let (prefix, rest) = match line.strip_prefix('[').and_then(|r| r.split_once("] ")) {
        Some((p, rest)) => (Some(p), rest),
        None => (None, line),
    };
    match rest.split_once(' ') {
        Some((t, text)) if kubectl::since_time(t).is_some() => (prefix, Some(t), text),
        _ => (prefix, None, rest),
    }
}

impl LogView {
    pub fn new(target: Target, context: Option<String>, containers: Vec<String>, container: Option<String>) -> LogView {
        LogView {
            target,
            context,
            containers,
            options: LogOptions { container, tail: TAIL, follow: true, ..LogOptions::default() },
            lines: VecDeque::new(),
            dropped: 0,
            stream: Stream::Starting,
            top: None,
            times: false,
            filter: String::new(),
            last_time: None,
        }
    }

    /// The command line for the stream, as the options are now.
    pub fn args(&self) -> Vec<String> {
        kubectl::logs(&self.target, self.context.as_deref(), &self.options)
    }

    /// A line the stream wrote.
    pub fn push(&mut self, line: &str) {
        let line = plain(line);
        if let (_, Some(t), _) = split(&line) {
            self.last_time = Some(t.to_string());
        }
        self.lines.push_back(line);
        while self.lines.len() > KEPT {
            self.lines.pop_front();
            self.dropped += 1;
            if let Some(top) = self.top.as_mut() {
                *top = top.saturating_sub(1);
            }
        }
    }

    /// Started again: from where it stopped, when it can say where.
    pub fn resume(&mut self) {
        self.options.since_time = self.last_time.clone();
        self.options.previous = false;
        self.stream = Stream::Starting;
    }

    /// Started again from scratch, with new options.
    pub fn restart(&mut self) {
        self.lines.clear();
        self.dropped = 0;
        self.options.since_time = None;
        self.last_time = None;
        self.top = None;
        self.stream = Stream::Starting;
    }

    fn shown(&self) -> Vec<&String> {
        self.lines.iter().filter(|l| self.filter.is_empty() || l.contains(&self.filter)).collect()
    }

    pub fn key(&mut self, key: Key, height: usize) -> LogAction {
        let n = self.shown().len();
        let view = height.saturating_sub(3).max(1);
        let last_top = n.saturating_sub(view);
        let top = self.top.unwrap_or(last_top);
        let move_to = |to: usize| if to >= last_top { None } else { Some(to) };
        match key {
            Key::Char('j') | Key::Down => self.top = move_to(top + 1),
            Key::Char('k') | Key::Up => self.top = move_to(top.saturating_sub(1)),
            Key::Char('J') | Key::PageDown => self.top = move_to(top + view),
            Key::Char('K') | Key::PageUp => self.top = move_to(top.saturating_sub(view)),
            Key::Char('g') | Key::Home => self.top = move_to(0),
            Key::Char('G') | Key::End | Key::Char('F') => self.top = None,
            Key::Char('t') => self.times = !self.times,
            Key::Char('s') => {
                return match self.stream {
                    Stream::Running | Stream::Starting => LogAction::Stop,
                    _ => {
                        self.resume();
                        LogAction::Restart
                    }
                }
            }
            Key::Char('p') => {
                self.options.previous = !self.options.previous;
                self.restart();
                return LogAction::Restart;
            }
            Key::Char('S') => return LogAction::AskSince,
            Key::Char('c') if self.containers.len() > 1 => return LogAction::PickContainer,
            Key::Char('a') if self.containers.len() > 1 || self.options.container.is_none() => {
                self.options.container = if self.options.container.is_some() { None } else { self.containers.first().cloned() };
                self.restart();
                return LogAction::Restart;
            }
            Key::Char('o') => return LogAction::OpenBuffer,
            Key::Char('/') => return LogAction::Filter,
            _ => return LogAction::Nothing,
        }
        LogAction::Redraw
    }

    /// The text as a buffer has it.
    pub fn text(&self) -> String {
        let mut out: Vec<String> = Vec::new();
        for l in self.shown() {
            let (prefix, time, text) = split(l);
            let mut line = String::new();
            if let Some(p) = prefix {
                line.push_str(&format!("[{p}] "));
            }
            if let (true, Some(t)) = (self.times, time) {
                line.push_str(t);
                line.push(' ');
            }
            line.push_str(text);
            out.push(line);
        }
        out.join("\n")
    }

    pub fn title(&self) -> String {
        let what = match &self.options.container {
            Some(c) => format!("{} · {c}", self.target),
            None => format!("{} · all containers", self.target),
        };
        format!("{what} · {}/{}", self.context.as_deref().unwrap_or("kubeconfig's context"), self.target.namespace)
    }

    pub fn render(&self, width: usize, height: usize) -> Canvas {
        let mut c = Canvas::new();
        let w = width.max(30);
        let h = height.max(5);
        let x = c.put(0, 0, "Logs  ", Role::Title);
        let x = c.put_fit(x, 0, w.saturating_sub(x), &self.title(), Role::Text);
        let (state, role) = match &self.stream {
            Stream::Starting => ("  starting…".to_string(), Role::Muted),
            Stream::Running if self.options.previous => ("  the previous run".to_string(), Role::Accent),
            Stream::Running => (if self.top.is_none() { "  following" } else { "  streaming, scrolled up (G follows)" }.to_string(), Role::Good),
            Stream::Stopped => ("  stopped: s starts it again".to_string(), Role::Warn),
            Stream::Ended(why) => (format!("  ended: {why}"), Role::Warn),
        };
        let x = c.put_fit(x, 0, w.saturating_sub(x), &state, role);
        let mut opts = Vec::new();
        if let Some(s) = &self.options.since {
            opts.push(format!("since {s}"));
        }
        if !self.filter.is_empty() {
            opts.push(format!("filter {}", self.filter));
        }
        if !opts.is_empty() {
            c.put_fit(x, 0, w.saturating_sub(x), &format!("  ({})", opts.join(", ")), Role::Muted);
        }
        c.rule(1, 0..w);
        let shown = self.shown();
        let view = h.saturating_sub(3).max(1);
        let top = self.top.unwrap_or(shown.len().saturating_sub(view)).min(shown.len().saturating_sub(view));
        if shown.is_empty() {
            let empty = match self.stream {
                Stream::Starting | Stream::Running => "Nothing yet",
                _ => "Nothing",
            };
            c.put(0, 2, empty, Role::Muted);
        }
        for (i, l) in shown.iter().skip(top).take(view).enumerate() {
            let y = 2 + i;
            let (prefix, time, text) = split(l);
            let mut x = 0;
            if let Some(p) = prefix {
                x = c.put(x, y, &fit(&format!("[{p}] "), 40), Role::Accent);
            }
            if let (true, Some(t)) = (self.times, time) {
                x = c.put(x, y, &format!("{} ", t.get(..23).unwrap_or(t)), Role::Muted);
            }
            c.put_fit(x, y, w.saturating_sub(x), text, line_role(text));
        }
        let keys = if self.containers.len() > 1 { "s stop/start  p previous  S since  c container  a all  t times  / filter  o buffer  G follow" } else { "s stop/start  p previous  S since  t times  / filter  o buffer  G follow" };
        c.put_fit(0, h - 1, w, keys, Role::Muted);
        c
    }
}

/// How a log line reads: an error, a warning, or just a line.
fn line_role(text: &str) -> Role {
    let upper = text.to_ascii_uppercase();
    if ["ERROR", "FATAL", "PANIC", "EXCEPTION"].iter().any(|w| upper.contains(w)) {
        Role::Bad
    } else if upper.contains("WARN") {
        Role::Warn
    } else if upper.contains("DEBUG") || upper.contains("TRACE") {
        Role::Muted
    } else {
        Role::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kubectl::Kind;

    fn view() -> LogView {
        LogView::new(Target::new(Kind::Pod, "shop", "web-1").unwrap(), Some("kind-dev".into()), vec!["nginx".into(), "sidecar".into()], Some("nginx".into()))
    }

    #[test]
    fn a_line_is_split_as_kubectl_wrote_it() {
        assert_eq!(split("2026-11-14T10:00:00.000000001Z GET / 200"), (None, Some("2026-11-14T10:00:00.000000001Z"), "GET / 200"));
        assert_eq!(split("[pod/web-1/nginx] 2026-11-14T10:00:00Z ready"), (Some("pod/web-1/nginx"), Some("2026-11-14T10:00:00Z"), "ready"));
        assert_eq!(split("no time here"), (None, None, "no time here"));
    }

    #[test]
    fn its_keys_restart_the_stream_with_their_options() {
        let mut v = view();
        assert_eq!(v.args(), ["logs", "--context=kind-dev", "--namespace=shop", "--container=nginx", "--timestamps=true", "--tail=200", "--follow=true", "web-1"]);
        assert_eq!(v.key(Key::Char('p'), 24), LogAction::Restart);
        assert!(v.args().contains(&"--previous=true".to_string()) && !v.args().iter().any(|a| a.starts_with("--follow")));
        v.key(Key::Char('p'), 24);
        assert_eq!(v.key(Key::Char('a'), 24), LogAction::Restart);
        assert!(v.args().contains(&"--all-containers=true".to_string()));
        assert_eq!(v.key(Key::Char('c'), 24), LogAction::PickContainer);
        v.stream = Stream::Running;
        v.push("2026-11-14T10:00:01Z one");
        assert_eq!(v.key(Key::Char('s'), 24), LogAction::Stop);
        v.stream = Stream::Stopped;
        assert_eq!(v.key(Key::Char('s'), 24), LogAction::Restart);
        assert!(v.args().contains(&"--since-time=2026-11-14T10:00:01Z".to_string()), "{:?}", v.args());
        assert!(!v.args().iter().any(|a| a.starts_with("--tail")));
    }

    #[test]
    fn it_keeps_its_last_lines_and_scrolls() {
        let mut v = view();
        for n in 0..KEPT + 10 {
            v.push(&format!("2026-11-14T10:00:00Z line {n}"));
        }
        assert_eq!(v.lines.len(), KEPT);
        assert_eq!(v.dropped, 10);
        assert!(v.render(80, 10).text().contains(&format!("line {}", KEPT + 9)), "following: the end shown");
        v.key(Key::Char('g'), 10);
        assert_eq!(v.top, Some(0));
        assert!(v.render(80, 10).text().contains("line 10\n"));
        v.key(Key::Char('G'), 10);
        assert_eq!(v.top, None);
        v.times = true;
        assert!(v.render(80, 10).text().contains("2026-11-14T10:00:00Z line"));
        v.push("\u{1b}[31mERROR\u{1b}[0m boom\u{7}");
        assert!(v.lines.back().unwrap() == "ERROR boom");
    }
}
