//! A call's answer, read from grpcurl's `-v` output as it comes, a line
//! at a time: the method it resolved, the metadata sent, the response's
//! headers, each message (pretty JSON, `Response contents:` before each),
//! its trailers, and on stderr the status when it isn't OK (`ERROR:`,
//! `Code:`, `Message:`, `Details:`) or why there was no call at all. A
//! server stream's messages are there as each arrives, with when it did.

/// A gRPC status code's name, by its number (grpcurl exits 64 + it).
pub const CODES: [&str; 17] = [
    "OK", "Canceled", "Unknown", "InvalidArgument", "DeadlineExceeded", "NotFound", "AlreadyExists", "PermissionDenied", "ResourceExhausted", "FailedPrecondition", "Aborted", "OutOfRange", "Unimplemented", "Internal", "Unavailable", "DataLoss", "Unauthenticated",
];

/// A message that came, and when: milliseconds after the call started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    pub at: u64,
    pub json: String,
}

/// How a call ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Still going.
    Running,
    /// A status from the server: `OK`, or the code and its message.
    Status { code: String, message: String, details: Vec<String> },
    /// No call was made, or grpcurl stopped: why, in its words.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub descriptor: Option<String>,
    pub sent_metadata: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub messages: Vec<Received>,
    pub trailers: Vec<(String, String)>,
    pub outcome: Outcome,
    /// `Sent 1 request and received 3 responses`.
    pub summary: Option<String>,
    /// When the headers came, and when it ended (ms after the start).
    pub headers_at: Option<u64>,
    pub ended_at: Option<u64>,
}

impl Default for Reply {
    fn default() -> Reply {
        Reply { descriptor: None, sent_metadata: Vec::new(), headers: Vec::new(), messages: Vec::new(), trailers: Vec::new(), outcome: Outcome::Running, summary: None, headers_at: None, ended_at: None }
    }
}

impl Reply {
    /// Its status code's name: `OK`, `NotFound`; `ERROR` when it never got
    /// one; empty while it runs.
    pub fn code(&self) -> &str {
        match &self.outcome {
            Outcome::Running => "",
            Outcome::Status { code, .. } => code,
            Outcome::Failed(_) => "ERROR",
        }
    }

    pub fn ok(&self) -> bool {
        self.code() == "OK"
    }

    pub fn running(&self) -> bool {
        self.outcome == Outcome::Running
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Section {
    #[default]
    None,
    Descriptor,
    Metadata,
    Headers,
    Contents,
    Trailers,
}

/// Reads one call's lines into its [`Reply`].
#[derive(Debug, Default)]
pub struct Reader {
    section: Section,
    /// The message being read, its lines so far.
    message: Vec<String>,
    /// On stderr: in `ERROR:`'s block, and in its `Details:`.
    error: bool,
    details: bool,
    code: Option<String>,
    status_message: String,
    detail_lines: Vec<String>,
    /// What else stderr said.
    said: Vec<String>,
}

fn pair(line: &str) -> Option<(String, String)> {
    let (k, v) = line.split_once(':')?;
    Some((k.trim().to_string(), v.trim().to_string()))
}

impl Reader {
    pub fn new() -> Reader {
        Reader::default()
    }

    /// A line grpcurl printed, `at` ms after the start. Whether the reply
    /// changed in a way worth drawing.
    pub fn line(&mut self, reply: &mut Reply, line: &str, stderr: bool, at: u64) -> bool {
        if stderr {
            return self.stderr(line);
        }
        let header = match line.trim_end() {
            "Resolved method descriptor:" => Some(Section::Descriptor),
            "Request metadata to send:" => Some(Section::Metadata),
            "Response headers received:" => Some(Section::Headers),
            "Response contents:" => Some(Section::Contents),
            "Response trailers received:" => Some(Section::Trailers),
            _ => None,
        };
        if let Some(s) = header {
            self.flush(reply, at);
            self.section = s;
            if s == Section::Headers {
                reply.headers_at = Some(at);
            }
            return false;
        }
        if line.starts_with("Sent ") && line.contains(" request") {
            self.flush(reply, at);
            reply.summary = Some(line.trim().to_string());
            self.section = Section::None;
            return true;
        }
        match self.section {
            Section::Descriptor => {
                if !line.trim().is_empty() {
                    reply.descriptor = Some(line.trim().to_string());
                }
                false
            }
            Section::Metadata | Section::Headers | Section::Trailers => {
                let l = line.trim();
                if l.is_empty() || l == "(empty)" {
                    return false;
                }
                let Some(kv) = pair(l) else { return false };
                match self.section {
                    Section::Metadata => reply.sent_metadata.push(kv),
                    Section::Headers => reply.headers.push(kv),
                    _ => reply.trailers.push(kv),
                }
                true
            }
            Section::Contents => {
                if line.trim().is_empty() && self.message.is_empty() {
                    return false;
                }
                self.message.push(line.to_string());
                // Pretty JSON: a message ends with its closing brace at
                // the start of a line (or is a value on one line).
                let one_line = self.message.len() == 1 && !line.trim_end().ends_with('{') && !line.trim_end().ends_with('[');
                if line.starts_with('}') || line.starts_with(']') || one_line {
                    self.flush(reply, at);
                    return true;
                }
                false
            }
            Section::None => false,
        }
    }

    fn stderr(&mut self, line: &str) -> bool {
        let l = line.trim();
        if l == "ERROR:" {
            self.error = true;
            return false;
        }
        if self.error {
            if let Some(code) = l.strip_prefix("Code:") {
                self.code = Some(code.trim().to_string());
                self.details = false;
                return false;
            }
            if let Some(m) = l.strip_prefix("Message:") {
                self.status_message = m.trim().to_string();
                self.details = false;
                return false;
            }
            if l == "Details:" {
                self.details = true;
                return false;
            }
            if self.details && !l.is_empty() {
                self.detail_lines.push(l.to_string());
                return false;
            }
        }
        if !l.is_empty() {
            self.said.push(l.to_string());
        }
        false
    }

    fn flush(&mut self, reply: &mut Reply, at: u64) {
        if self.message.is_empty() {
            return;
        }
        let json = std::mem::take(&mut self.message).join("\n");
        reply.messages.push(Received { at, json });
    }

    /// grpcurl ended, with `code`, `at` ms after the start.
    pub fn end(&mut self, reply: &mut Reply, code: Option<i32>, at: u64) {
        self.flush(reply, at);
        reply.ended_at = Some(at);
        reply.outcome = match (&self.code, code) {
            (Some(c), _) => Outcome::Status { code: c.clone(), message: self.status_message.clone(), details: self.detail_lines.clone() },
            (None, Some(0)) => Outcome::Status { code: "OK".into(), message: String::new(), details: Vec::new() },
            // 64 + the code, when it didn't print it.
            (None, Some(c)) if (65..64 + CODES.len() as i32).contains(&c) && reply.summary.is_some() => {
                Outcome::Status { code: CODES[(c - 64) as usize].into(), message: self.said.join(" "), details: Vec::new() }
            }
            (None, c) => {
                let why = if self.said.is_empty() {
                    match c {
                        Some(c) => format!("grpcurl exited with {c}"),
                        None => "it was stopped".into(),
                    }
                } else {
                    self.said.join("\n")
                };
                Outcome::Failed(why)
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sample as grpcurl printed it: stdout, then (after `ERROR:`) what
    /// it said on stderr, each line `ms` after the last.
    fn read(sample: &str, code: i32, ms: u64) -> Reply {
        let mut reply = Reply::default();
        let mut r = Reader::new();
        let mut at = 0;
        let mut stderr = false;
        for line in sample.lines() {
            stderr |= line == "ERROR:" || line.starts_with("Failed to") || line.starts_with("Error invoking");
            at += ms;
            r.line(&mut reply, line, stderr, at);
        }
        r.end(&mut reply, Some(code), at + ms);
        reply
    }

    #[test]
    fn a_unary_call_is_its_message_headers_and_trailers() {
        let r = read(include_str!("../samples/grpcurl-unary.txt"), 0, 1);
        assert_eq!(r.descriptor.as_deref(), Some("rpc GetSatellite ( .orbit.v1.GetSatelliteRequest ) returns ( .orbit.v1.GetSatelliteResponse );"));
        assert_eq!(r.sent_metadata, [("x-team".to_string(), "ops".to_string())]);
        assert_eq!(r.headers.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["content-type", "grpc-accept-encoding", "x-served-by"]);
        assert_eq!(r.messages.len(), 1);
        let body: serde_json::Value = serde_json::from_str(&r.messages[0].json).unwrap();
        assert_eq!(body["satellite"]["name"], "ISS ops");
        assert_eq!(r.trailers, [("x-cost".to_string(), "3".to_string())]);
        assert_eq!((r.code(), r.summary.as_deref()), ("OK", Some("Sent 1 request and received 1 response")));
        assert!(r.headers_at.unwrap() < r.messages[0].at && r.messages[0].at < r.ended_at.unwrap());
    }

    #[test]
    fn a_server_stream_has_each_message_as_it_came() {
        let mut reply = Reply::default();
        let mut r = Reader::new();
        let mut firsts = Vec::new();
        for (i, line) in include_str!("../samples/grpcurl-stream.txt").lines().enumerate() {
            if r.line(&mut reply, line, false, i as u64 * 10) && !reply.messages.is_empty() {
                firsts.push(reply.messages.len());
            }
        }
        assert_eq!(reply.messages.len(), 3, "all three before it ends");
        assert!(reply.running());
        assert!(reply.messages.windows(2).all(|w| w[0].at < w[1].at));
        assert!(reply.messages[2].json.contains("\"max_elevation\": 31.5"));
        r.end(&mut reply, Some(0), 1000);
        assert_eq!((reply.code(), reply.trailers.len()), ("OK", 0), "(empty) trailers");
    }

    #[test]
    fn an_error_is_its_status_and_a_failure_its_words() {
        let r = read(include_str!("../samples/grpcurl-error.txt"), 69, 1);
        assert_eq!(r.outcome, Outcome::Status { code: "NotFound".into(), message: "no satellite missing".into(), details: Vec::new() });
        assert!(!r.ok() && r.messages.is_empty());
        assert_eq!(r.trailers, [("x-cost".to_string(), "3".to_string())], "trailers with an error too");

        let dial = read("Failed to dial target host \"127.0.0.1:50999\": context deadline exceeded", 1, 1);
        assert_eq!(dial.outcome, Outcome::Failed("Failed to dial target host \"127.0.0.1:50999\": context deadline exceeded".into()));
        assert_eq!(dial.code(), "ERROR");

        let mut reply = Reply::default();
        let mut r = Reader::new();
        for l in ["ERROR:", "  Code: InvalidArgument", "  Message: bad id", "  Details:", "  1)\t{\"@type\": \"type.googleapis.com/google.rpc.BadRequest\"}"] {
            r.line(&mut reply, l, true, 1);
        }
        r.end(&mut reply, Some(67), 2);
        assert!(matches!(&reply.outcome, Outcome::Status { code, details, .. } if code == "InvalidArgument" && details.len() == 1));
        let mut stopped = Reply::default();
        Reader::new().end(&mut stopped, None, 5);
        assert_eq!(stopped.outcome, Outcome::Failed("it was stopped".into()));
        assert_eq!(CODES[5], "NotFound");
    }
}
