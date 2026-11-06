//! Inboxes: the subjects replies come back on.
//!
//! A client subscribes once to `_INBOX.<its own>.*`, and gives each
//! request a token of its own under it ([`Requests`]): a reply's subject
//! says which request it answers, and nothing needs subscribing per
//! request.

use std::collections::HashMap;

const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Unique names, as NATS's NUID makes them: a random prefix and a counter,
/// 22 characters of base 62. Seeded by the caller -- this crate has no
/// clock or randomness of its own.
#[derive(Debug, Clone)]
pub struct Nuid {
    state: u64,
    prefix: [u8; 12],
    seq: u64,
}

impl Nuid {
    pub fn new(seed: u64) -> Nuid {
        let mut n = Nuid { state: seed ^ 0x9e37_79b9_7f4a_7c15, prefix: [b'0'; 12], seq: 0 };
        n.reseed();
        n
    }

    fn random(&mut self) -> u64 {
        // splitmix64
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn reseed(&mut self) {
        for i in 0..12 {
            let r = self.random();
            self.prefix[i] = DIGITS[(r % 62) as usize];
        }
        self.seq = self.random() % 839_299_365_868_340_224; // 62^10
    }

    /// The next one.
    pub fn next_id(&mut self) -> String {
        self.seq += 1;
        if self.seq >= 839_299_365_868_340_224 {
            self.reseed();
        }
        let mut out = Vec::with_capacity(22);
        out.extend_from_slice(&self.prefix);
        let mut n = self.seq;
        let mut tail = [b'0'; 10];
        for i in (0..10).rev() {
            tail[i] = DIGITS[(n % 62) as usize];
            n /= 62;
        }
        out.extend_from_slice(&tail);
        String::from_utf8(out).expect("ASCII")
    }
}

/// Requests waiting for their replies, under one inbox subscription.
#[derive(Debug)]
pub struct Requests<T> {
    /// `_INBOX.<nuid>`; replies come to `<prefix>.<token>`.
    pub prefix: String,
    next: u64,
    waiting: HashMap<u64, (T, u64)>,
}

impl<T> Requests<T> {
    pub fn new(prefix: &str) -> Requests<T> {
        Requests { prefix: prefix.to_string(), next: 0, waiting: HashMap::new() }
    }

    /// An inbox from `nuid`: `_INBOX.<nuid>`.
    pub fn with_nuid(nuid: &mut Nuid) -> Requests<T> {
        Requests::new(&format!("_INBOX.{}", nuid.next_id()))
    }

    /// What the client subscribes to, once.
    pub fn subscription(&self) -> String {
        format!("{}.*", self.prefix)
    }

    /// A request waiting until `deadline` (the caller's clock); the subject
    /// its reply comes to.
    pub fn add(&mut self, what: T, deadline: u64) -> String {
        self.next += 1;
        self.waiting.insert(self.next, (what, deadline));
        format!("{}.{}", self.prefix, self.next)
    }

    /// The request a reply on `subject` answers, taken off the list; none
    /// when it's not one of these, or gave up already.
    pub fn take(&mut self, subject: &str) -> Option<T> {
        let token = subject.strip_prefix(self.prefix.as_str())?.strip_prefix('.')?;
        self.waiting.remove(&token.parse().ok()?).map(|(t, _)| t)
    }

    /// Whether `subject` is under this inbox.
    pub fn owns(&self, subject: &str) -> bool {
        subject.strip_prefix(self.prefix.as_str()).is_some_and(|r| r.starts_with('.'))
    }

    /// Those past their deadline by `now`, taken off.
    pub fn expired(&mut self, now: u64) -> Vec<T> {
        let gone: Vec<u64> = self.waiting.iter().filter(|(_, (_, d))| *d <= now).map(|(k, _)| *k).collect();
        gone.into_iter().filter_map(|k| self.waiting.remove(&k)).map(|(t, _)| t).collect()
    }

    /// Every one still waiting, taken off: the connection dropped.
    pub fn drain(&mut self) -> Vec<T> {
        self.waiting.drain().map(|(_, (t, _))| t).collect()
    }

    pub fn len(&self) -> usize {
        self.waiting.len()
    }

    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nuids_are_unique_and_22_long() {
        let mut n = Nuid::new(42);
        let a = n.next_id();
        let b = n.next_id();
        assert_eq!(a.len(), 22);
        assert_ne!(a, b);
        assert_eq!(a[..12], b[..12]);
        assert_ne!(Nuid::new(1).next_id(), Nuid::new(2).next_id());
    }

    #[test]
    fn replies_find_their_requests() {
        let mut r: Requests<&str> = Requests::new("_INBOX.abc");
        assert_eq!(r.subscription(), "_INBOX.abc.*");
        let one = r.add("one", 100);
        let two = r.add("two", 200);
        assert!(r.owns(&one) && !r.owns("_INBOX.abcd.1") && !r.owns("other"));
        assert_eq!(r.take(&two), Some("two"));
        assert_eq!(r.take(&two), None);
        assert_eq!(r.take("_INBOX.abc.x"), None);
        assert_eq!(r.expired(99), Vec::<&str>::new());
        assert_eq!(r.expired(100), vec!["one"]);
        assert!(r.is_empty());
        r.add("three", 5);
        assert_eq!(r.drain(), vec!["three"]);
    }
}
