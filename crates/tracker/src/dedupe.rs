//! Writes already done, by their call and `op`: a tracker that keeps this
//! answers a write sent again (its caller gave up waiting, and the user
//! said send it again) as it answered the first, without doing it twice.
//! The same `op` on another call is another write; the same call and
//! `op` with another payload isn't a repeat, and is refused. Kept for as
//! long as the tracker's extension runs, the most recent [`Dedupe::KEEP`].
//! A write still under way isn't here yet: sent again meanwhile, it runs
//! twice.

use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};

use crate::{code, SeamError};

#[derive(Debug, Clone, Default)]
pub struct Dedupe {
    done: VecDeque<(String, String, u64, Result<String, String>)>,
}

fn digest(payload: &str) -> u64 {
    let mut h = DefaultHasher::new();
    payload.hash(&mut h);
    h.finish()
}

impl Dedupe {
    pub const KEEP: usize = 500;

    /// What write `call` with `op` was answered, when it's been done --
    /// or a refusal, when that `op` was another payload's.
    pub fn get(&self, call: &str, op: Option<&str>, payload: &str) -> Option<Result<String, String>> {
        let op = op?;
        let (_, _, sum, answer) = self.done.iter().find(|(c, o, _, _)| c == call && o == op)?;
        if *sum != digest(payload) {
            return Some(Err(SeamError::new(code::BAD_REQUEST, format!("op {op} was another change's")).to_err()));
        }
        Some(answer.clone())
    }

    /// Write `call` with `op` and `payload` was answered `answer`: kept
    /// when it was done (an error means it wasn't, so it may be tried
    /// again).
    pub fn put(&mut self, call: &str, op: Option<&str>, payload: &str, answer: &Result<String, String>) {
        let (Some(op), Ok(_)) = (op, answer) else { return };
        if self.done.len() == Self::KEEP {
            self.done.pop_front();
        }
        self.done.push_back((call.to_string(), op.to_string(), digest(payload), answer.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_done_is_answered_again_and_a_failure_isnt_kept() {
        let mut d = Dedupe::default();
        assert_eq!(d.get("comment", Some("op-1"), "{a}"), None);
        d.put("comment", Some("op-1"), "{a}", &Ok("{}".into()));
        d.put("comment", Some("op-2"), "{b}", &Err("HTTP 500".into()));
        d.put("comment", None, "{c}", &Ok("{}".into()));
        assert_eq!(d.get("comment", Some("op-1"), "{a}"), Some(Ok("{}".into())));
        assert_eq!(d.get("comment", Some("op-2"), "{b}"), None);
        assert_eq!(d.get("comment", None, "{c}"), None);
    }

    #[test]
    fn the_same_op_on_another_call_or_payload_isnt_a_repeat() {
        let mut d = Dedupe::default();
        d.put("comment", Some("agenda:1:42"), r#"{"body":"x"}"#, &Ok("{}".into()));
        assert_eq!(d.get("transition", Some("agenda:1:42"), r#"{"id":"31"}"#), None, "a move with a comment's op is still made");
        let other = d.get("comment", Some("agenda:1:42"), r#"{"body":"y"}"#).unwrap().unwrap_err();
        assert!(other.contains("bad_request"));
    }
}
