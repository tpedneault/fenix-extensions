//! The seam's errors: an `Err` is a JSON object as text, `{"code",
//! "message", "retry_after"}` -- and anything else an `Err` can be
//! (Fenix's own, an older tracker's plain text) is read as one too.

use serde::{Deserialize, Serialize};

/// The codes an error can have (`plans/tracker-seam.md` §3.3).
pub mod code {
    /// No server, workspace or sign-in configured.
    pub const NOT_SET_UP: &str = "not_set_up";
    /// The sign-in was refused.
    pub const SIGNED_OUT: &str = "signed_out";
    /// The issue isn't there, or you can't see it.
    pub const NOT_FOUND: &str = "not_found";
    /// Slow down, for `retry_after` seconds.
    pub const RATE_LIMITED: &str = "rate_limited";
    /// The request didn't reach the tracker.
    pub const OFFLINE: &str = "offline";
    /// The tracker said no.
    pub const REFUSED: &str = "refused";
    /// The request wasn't understood.
    pub const BAD_REQUEST: &str = "bad_request";
    /// A call, capability or seam version it doesn't serve.
    pub const UNSUPPORTED: &str = "unsupported";
    /// Anything else.
    pub const FAILED: &str = "failed";

    pub const ALL: [&str; 9] = [NOT_SET_UP, SIGNED_OUT, NOT_FOUND, RATE_LIMITED, OFFLINE, REFUSED, BAD_REQUEST, UNSUPPORTED, FAILED];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeamError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
}

impl SeamError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        SeamError { code: code.to_string(), message: crate::clean::message(&message.into()), retry_after: None }
    }

    pub fn is(&self, code: &str) -> bool {
        self.code == code
    }

    /// Worth trying again by itself, later: the next timed sync does.
    pub fn transient(&self) -> bool {
        self.is(code::OFFLINE) || self.is(code::RATE_LIMITED)
    }

    /// Fenix's answer for a tracker that isn't installed.
    pub fn absent(&self) -> bool {
        self.is(code::UNSUPPORTED) && self.message.starts_with("nothing offers ")
    }

    /// As an `Err` carries it.
    pub fn to_err(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| self.message.clone())
    }

    /// An `Err` as it came: the seam's JSON, or Fenix's words (`nothing
    /// offers`, `no service`, `the extension stopped`), or an older
    /// tracker's text, which is `failed`.
    pub fn from_err(text: &str) -> SeamError {
        if let Ok(mut err) = serde_json::from_str::<SeamError>(text) {
            if !code::ALL.contains(&err.code.as_str()) {
                err.code = code::FAILED.into();
            }
            err.message = crate::clean::message(&err.message);
            return err;
        }
        let code = if text.starts_with("nothing offers ") || text.starts_with("no service ") {
            code::UNSUPPORTED
        } else if text.starts_with("the extension stopped") {
            code::OFFLINE
        } else {
            code::FAILED
        };
        SeamError::new(code, text)
    }
}

impl std::fmt::Display for SeamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_round_trips_as_json() {
        let mut err = SeamError::new(code::RATE_LIMITED, "HTTP 429");
        err.retry_after = Some(30);
        assert_eq!(err.to_err(), r#"{"code":"rate_limited","message":"HTTP 429","retry_after":30}"#);
        assert_eq!(SeamError::from_err(&err.to_err()), err);
        assert!(err.transient());
    }

    #[test]
    fn fenixs_own_words_are_read_as_codes() {
        let gone = SeamError::from_err("nothing offers jira.tracker.about");
        assert!(gone.is(code::UNSUPPORTED) && gone.absent());
        let older = SeamError::from_err("no service jira.tracker.worklog");
        assert!(older.is(code::UNSUPPORTED) && !older.absent());
        assert!(SeamError::from_err("the extension stopped: it took too long").is(code::OFFLINE));
        let plain = SeamError::from_err("HTTP 500");
        assert_eq!((plain.code.as_str(), plain.message.as_str()), ("failed", "HTTP 500"));
    }

    #[test]
    fn an_unknown_code_is_failed_and_the_message_is_cleaned() {
        let err = SeamError::from_err(r#"{"code": "kaboom", "message": "bad\u001b[31m\nnews"}"#);
        assert_eq!(err.code, "failed");
        assert_eq!(err.message, "bad[31m news");
    }
}
