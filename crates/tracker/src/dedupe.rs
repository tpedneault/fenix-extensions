//! Writes already done, by their `op`: a tracker that keeps this answers
//! a write sent again (its caller gave up waiting, and the user said send
//! it again) as it answered the first, without doing it twice. Kept for
//! the session, the most recent [`Dedupe::KEEP`].

use std::collections::VecDeque;

#[derive(Debug, Clone, Default)]
pub struct Dedupe {
    done: VecDeque<(String, Result<String, String>)>,
}

impl Dedupe {
    pub const KEEP: usize = 500;

    /// What write `op` was answered, when it's been done.
    pub fn get(&self, op: Option<&str>) -> Option<Result<String, String>> {
        let op = op?;
        self.done.iter().find(|(o, _)| o == op).map(|(_, a)| a.clone())
    }

    /// Write `op` was answered `answer`: kept when it was done (an
    /// error means it wasn't, so it may be tried again).
    pub fn put(&mut self, op: Option<&str>, answer: &Result<String, String>) {
        let (Some(op), Ok(_)) = (op, answer) else { return };
        if self.done.len() == Self::KEEP {
            self.done.pop_front();
        }
        self.done.push_back((op.to_string(), answer.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_done_is_answered_again_and_a_failure_isnt_kept() {
        let mut d = Dedupe::default();
        assert_eq!(d.get(Some("op-1")), None);
        d.put(Some("op-1"), &Ok("{}".into()));
        d.put(Some("op-2"), &Err("HTTP 500".into()));
        d.put(None, &Ok("{}".into()));
        assert_eq!(d.get(Some("op-1")), Some(Ok("{}".into())));
        assert_eq!(d.get(Some("op-2")), None);
        assert_eq!(d.get(None), None);
    }
}
