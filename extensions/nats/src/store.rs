//! What came, kept within a budget: every message counted, the recent ones
//! stored as they came -- raw bytes, read only when you look.
//!
//! Each subject has its counters (messages, bytes, when it was last seen)
//! and a rate a second for the last minute: a few additions a message.
//! What a message held is copied into blocks of 1 MiB, one after the other,
//! so the store's memory is its budget and no more, and keeping a message
//! allocates nothing; a subject keeps a short entry for each of its own,
//! pointing into them. The oldest block goes when a new one is wanted and
//! the budget is spent, and its messages with it; a subject also gives up
//! its oldest past its own caps (how many, how many bytes).
//!
//! Under load the store keeps 1 message in N of a busy subject's (`sampling`)
//! while every one is still counted; a quiet subject keeps them all.

use std::collections::VecDeque;

use rustc_hash::FxHashMap;

/// How big a block is: the most a message keeps.
pub const BLOCK: usize = 1 << 20;

/// A subject sending fewer than this a second keeps every message, however
/// busy the rest are.
pub const QUIET: u32 = 20;

/// The rate's history, in seconds.
pub const SECONDS: usize = 60;

/// Its limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Bytes the blocks may take, all subjects together.
    pub budget: usize,
    /// Messages a subject keeps.
    pub per_subject: usize,
    /// Bytes a subject keeps.
    pub per_subject_bytes: usize,
    /// Subjects counted one by one; more are counted together.
    pub max_subjects: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { budget: 64 << 20, per_subject: 2000, per_subject_bytes: 4 << 20, max_subjects: 5000 }
    }
}

impl Limits {
    /// Entries kept at most, all subjects together: about 64 bytes of
    /// bookkeeping for each 256 bytes of budget.
    pub fn max_entries(&self) -> usize {
        (self.budget / 256).max(1024)
    }
}

/// Messages a second, for the last minute.
#[derive(Debug, Clone)]
pub struct Rate {
    /// The second the last bucket is for.
    sec: u64,
    buckets: [u32; SECONDS],
}

impl Default for Rate {
    fn default() -> Rate {
        Rate { sec: 0, buckets: [0; SECONDS] }
    }
}

impl Rate {
    #[inline]
    pub fn hit(&mut self, now_sec: u64, n: u32) {
        if now_sec != self.sec {
            self.roll(now_sec);
        }
        let b = &mut self.buckets[(now_sec % SECONDS as u64) as usize];
        *b = b.saturating_add(n);
    }

    #[cold]
    fn roll(&mut self, now_sec: u64) {
        if now_sec < self.sec {
            return;
        }
        let gap = (now_sec - self.sec).min(SECONDS as u64);
        for i in 1..=gap {
            self.buckets[((self.sec + i) % SECONDS as u64) as usize] = 0;
        }
        self.sec = now_sec;
    }

    /// What second `sec` had, as seen at `now_sec`.
    fn at(&self, sec: u64, now_sec: u64) -> u32 {
        if sec > self.sec || self.sec >= sec + SECONDS as u64 || now_sec >= sec + SECONDS as u64 {
            0
        } else {
            self.buckets[(sec % SECONDS as u64) as usize]
        }
    }

    /// The messages this second so far.
    pub fn current(&self, now_sec: u64) -> u32 {
        self.at(now_sec, now_sec)
    }

    /// A second's rate: the last three whole seconds', averaged.
    pub fn per_sec(&self, now_sec: u64) -> f64 {
        let sum: u32 = (1..=3).filter_map(|i| now_sec.checked_sub(i)).map(|s| self.at(s, now_sec)).sum();
        sum as f64 / 3.0
    }

    /// The last `n` whole seconds, oldest first: a sparkline.
    pub fn series(&self, now_sec: u64, n: usize) -> Vec<u32> {
        let n = n.min(SECONDS - 1);
        (1..=n as u64).rev().map(|i| now_sec.checked_sub(i).map_or(0, |s| self.at(s, now_sec))).collect()
    }
}

/// A stored message's entry: where its bytes are, and what's shown of it
/// without reading them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Its number among those stored: one a message, all subjects counting.
    pub seq: u32,
    block: u32,
    off: u32,
    /// Bytes kept: the reply subject, headers and payload, as far as they
    /// fitted.
    len: u32,
    /// Its size on the wire, headers and payload.
    pub size: u32,
    /// Milliseconds since the store's epoch.
    pub time: u32,
    hdr_len: u32,
    reply_len: u16,
    pub header_count: u16,
    /// Cut short to fit a block.
    pub truncated: bool,
}

/// A stored message, borrowed from its block.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    pub entry: Entry,
    pub subject: &'a str,
    pub reply: Option<&'a str>,
    pub headers: Option<&'a [u8]>,
    pub payload: &'a [u8],
    /// Unix milliseconds.
    pub time_ms: u64,
}

/// One subject of one connection.
#[derive(Debug)]
pub struct Subject {
    pub conn: u16,
    pub name: Box<str>,
    pub count: u64,
    pub bytes: u64,
    /// Unix milliseconds.
    pub first_ms: u64,
    pub last_ms: u64,
    pub rate: Rate,
    pub msgs: VecDeque<Entry>,
    pub stored_bytes: usize,
    /// Counted, not stored: left out by sampling or while paused.
    pub skipped: u64,
    /// Stored, then let go for room.
    pub evicted: u64,
    /// Its bit for each highlight rule whose subject it matches, worked out
    /// when it first comes and when the rules change.
    pub rule_mask: u64,
    /// The generation of the rules `rule_mask` is for.
    pub rule_gen: u64,
    /// Whether this is where subjects past `max_subjects` are counted.
    pub overflow: bool,
}

impl Subject {
    fn new(conn: u16, name: &str, now_ms: u64) -> Subject {
        Subject {
            conn,
            name: name.into(),
            count: 0,
            bytes: 0,
            first_ms: now_ms,
            last_ms: now_ms,
            rate: Rate::default(),
            msgs: VecDeque::new(),
            stored_bytes: 0,
            skipped: 0,
            evicted: 0,
            rule_mask: 0,
            rule_gen: u64::MAX,
            overflow: false,
        }
    }

    pub fn average_size(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.bytes as f64 / self.count as f64
        }
    }

    /// Where the entry `seq` is among its messages, oldest first.
    pub fn position(&self, seq: u32) -> Option<usize> {
        let front = self.msgs.front()?.seq;
        let key = seq.wrapping_sub(front);
        let i = self.msgs.partition_point(|e| e.seq.wrapping_sub(front) < key);
        (self.msgs.get(i)?.seq == seq).then_some(i)
    }
}

/// Totals, all subjects together.
#[derive(Debug, Default, Clone)]
pub struct Totals {
    pub msgs: u64,
    pub bytes: u64,
    pub stored: u64,
    /// Counted, not stored (sampling, pause).
    pub skipped: u64,
    pub evicted: u64,
    pub rate: Rate,
    /// Each connection's: messages, bytes, rate.
    pub conns: Vec<(u64, u64, Rate)>,
}

#[derive(Debug)]
struct Block {
    id: u32,
    data: Vec<u8>,
}

/// Everything stored.
#[derive(Debug)]
pub struct Store {
    pub limits: Limits,
    /// Unix milliseconds that entries' times count from.
    epoch_ms: u64,
    blocks: VecDeque<Block>,
    next_block: u32,
    spare: Option<Vec<u8>>,
    /// Every stored message's (subject, seq), oldest first: what goes when
    /// a block does, or past the entries' cap. Some are of messages a
    /// subject let go already.
    fifo: VecDeque<(u32, u32)>,
    seq: u32,
    pub subjects: Vec<Subject>,
    index: Vec<FxHashMap<Box<str>, u32>>,
    overflow: Vec<Option<u32>>,
    pub totals: Totals,
    /// Keep 1 in this many of a busy subject's messages (1: all).
    pub sampling: u32,
    /// Counting only: nothing stored.
    pub paused: bool,
    /// Bumped when a subject is added: a tree or a sorted list is made again.
    pub subjects_gen: u64,
}

impl Store {
    pub fn new(limits: Limits, epoch_ms: u64) -> Store {
        Store {
            limits,
            epoch_ms,
            blocks: VecDeque::new(),
            next_block: 0,
            spare: None,
            fifo: VecDeque::with_capacity(limits.max_entries().min(1 << 16)),
            seq: 0,
            subjects: Vec::new(),
            index: Vec::new(),
            overflow: Vec::new(),
            totals: Totals::default(),
            sampling: 1,
            paused: false,
            subjects_gen: 0,
        }
    }

    /// Bytes its blocks take.
    pub fn memory(&self) -> usize {
        self.blocks.len() * BLOCK + self.spare.as_ref().map_or(0, |_| BLOCK) + self.fifo.capacity() * 8 + self.subjects.iter().map(|s| s.msgs.capacity() * std::mem::size_of::<Entry>() + 200).sum::<usize>()
    }

    /// Bytes its messages keep.
    pub fn stored_bytes(&self) -> usize {
        self.subjects.iter().map(|s| s.stored_bytes).sum()
    }

    pub fn entries(&self) -> usize {
        self.subjects.iter().map(|s| s.msgs.len()).sum()
    }

    /// Forgets every message and subject.
    pub fn clear(&mut self) {
        let limits = self.limits;
        let epoch = self.epoch_ms;
        let sampling = self.sampling;
        let paused = self.paused;
        let gen = self.subjects_gen + 1;
        *self = Store::new(limits, epoch);
        self.sampling = sampling;
        self.paused = paused;
        self.subjects_gen = gen;
    }

    /// New limits: what's past them goes now.
    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
        while self.blocks.len() * BLOCK > limits.budget.max(BLOCK) {
            self.drop_block();
        }
        for i in 0..self.subjects.len() {
            self.trim(i as u32);
        }
        self.trim_fifo();
    }

    /// The subject `name` of connection `conn`, made if it's new.
    #[inline]
    pub fn subject(&mut self, conn: u16, name: &str, now_ms: u64) -> u32 {
        let c = conn as usize;
        if self.index.len() <= c {
            self.index.resize_with(c + 1, FxHashMap::default);
            self.overflow.resize(c + 1, None);
            self.totals.conns.resize_with(c + 1, Default::default);
        }
        if let Some(&id) = self.index[c].get(name) {
            return id;
        }
        self.add_subject(conn, name, now_ms)
    }

    #[cold]
    fn add_subject(&mut self, conn: u16, name: &str, now_ms: u64) -> u32 {
        let c = conn as usize;
        if self.index[c].len() >= self.limits.max_subjects {
            if let Some(id) = self.overflow[c] {
                return id;
            }
            let id = self.subjects.len() as u32;
            let mut s = Subject::new(conn, "…", now_ms);
            s.overflow = true;
            self.subjects.push(s);
            self.overflow[c] = Some(id);
            self.subjects_gen += 1;
            return id;
        }
        let id = self.subjects.len() as u32;
        self.subjects.push(Subject::new(conn, name, now_ms));
        self.index[c].insert(name.into(), id);
        self.subjects_gen += 1;
        id
    }

    /// The subject `name` of `conn`, if it's come.
    pub fn find(&self, conn: u16, name: &str) -> Option<u32> {
        self.index.get(conn as usize)?.get(name).copied()
    }

    /// Counts a message on subject `id`; whether it's to be stored, as
    /// sampling and pausing say.
    #[inline]
    pub fn count(&mut self, id: u32, size: usize, now_ms: u64) -> bool {
        let now_sec = now_ms / 1000;
        let s = &mut self.subjects[id as usize];
        s.count += 1;
        s.bytes += size as u64;
        s.last_ms = now_ms;
        s.rate.hit(now_sec, 1);
        self.totals.msgs += 1;
        self.totals.bytes += size as u64;
        self.totals.rate.hit(now_sec, 1);
        let c = &mut self.totals.conns[s.conn as usize];
        c.0 += 1;
        c.1 += size as u64;
        c.2.hit(now_sec, 1);
        let keep = !self.paused && (self.sampling <= 1 || s.rate.current(now_sec) <= QUIET || s.count.is_multiple_of(self.sampling as u64));
        if !keep {
            s.skipped += 1;
            self.totals.skipped += 1;
        }
        keep
    }

    /// Stores a message counted on subject `id`.
    #[allow(clippy::too_many_arguments)]
    pub fn store(&mut self, id: u32, reply: Option<&str>, headers: Option<&[u8]>, header_count: usize, payload: &[u8], now_ms: u64) -> u32 {
        let reply = reply.map_or(&[][..], str::as_bytes);
        let reply_len = reply.len().min(u16::MAX as usize).min(BLOCK / 4);
        let reply = &reply[..reply_len];
        let headers = headers.unwrap_or(&[]);
        let size = headers.len() + payload.len();
        let room = BLOCK - reply_len;
        let hdr_len = headers.len().min(room);
        let pay_len = payload.len().min(room - hdr_len);
        let len = reply_len + hdr_len + pay_len;
        let block = self.room_for(len);
        let b = self.blocks.back_mut().expect("a block");
        let off = b.data.len();
        b.data.extend_from_slice(reply);
        b.data.extend_from_slice(&headers[..hdr_len]);
        b.data.extend_from_slice(&payload[..pay_len]);
        self.seq = self.seq.wrapping_add(1);
        let entry = Entry {
            seq: self.seq,
            block,
            off: off as u32,
            len: len as u32,
            size: size.min(u32::MAX as usize) as u32,
            time: now_ms.saturating_sub(self.epoch_ms).min(u32::MAX as u64) as u32,
            hdr_len: hdr_len as u32,
            reply_len: reply_len as u16,
            header_count: header_count.min(u16::MAX as usize) as u16,
            truncated: len < reply_len + size,
        };
        let s = &mut self.subjects[id as usize];
        s.msgs.push_back(entry);
        s.stored_bytes += len;
        self.totals.stored += 1;
        self.fifo.push_back((id, self.seq));
        self.trim(id);
        self.trim_fifo();
        self.seq
    }

    /// A block with `len` bytes free at the back, its id.
    #[inline]
    fn room_for(&mut self, len: usize) -> u32 {
        if let Some(b) = self.blocks.back() {
            if b.data.len() + len <= BLOCK {
                return b.id;
            }
        }
        self.new_block()
    }

    #[cold]
    fn new_block(&mut self) -> u32 {
        let max = (self.limits.budget / BLOCK).max(1);
        while self.blocks.len() >= max {
            self.drop_block();
        }
        let mut data = self.spare.take().unwrap_or_else(|| Vec::with_capacity(BLOCK));
        data.clear();
        let id = self.next_block;
        self.next_block = self.next_block.wrapping_add(1);
        self.blocks.push_back(Block { id, data });
        id
    }

    /// The oldest block goes, and its messages.
    fn drop_block(&mut self) {
        let Some(old) = self.blocks.pop_front() else { return };
        while let Some(&(sid, seq)) = self.fifo.front() {
            let s = &mut self.subjects[sid as usize];
            match s.msgs.front() {
                Some(e) if e.seq == seq => {
                    if e.block != old.id {
                        break;
                    }
                    s.stored_bytes -= e.len as usize;
                    s.evicted += 1;
                    s.msgs.pop_front();
                    self.totals.evicted += 1;
                }
                _ => {}
            }
            self.fifo.pop_front();
        }
        self.spare = Some(old.data);
    }

    /// Subject `id` within its own caps.
    fn trim(&mut self, id: u32) {
        let (count, bytes) = (self.limits.per_subject.max(1), self.limits.per_subject_bytes);
        let s = &mut self.subjects[id as usize];
        while s.msgs.len() > count || (s.stored_bytes > bytes && s.msgs.len() > 1) {
            let e = s.msgs.pop_front().expect("one");
            s.stored_bytes -= e.len as usize;
            s.evicted += 1;
            self.totals.evicted += 1;
        }
    }

    fn trim_fifo(&mut self) {
        let max = self.limits.max_entries();
        while self.fifo.len() > max {
            let (sid, seq) = self.fifo.pop_front().expect("one");
            let s = &mut self.subjects[sid as usize];
            if let Some(e) = s.msgs.pop_front_if(|e| e.seq == seq) {
                s.stored_bytes -= e.len as usize;
                s.evicted += 1;
                self.totals.evicted += 1;
            }
        }
    }

    /// The message an entry of subject `id` is.
    pub fn view(&self, id: u32, e: &Entry) -> Option<View<'_>> {
        let first = self.blocks.front()?.id;
        let b = self.blocks.get(e.block.wrapping_sub(first) as usize)?;
        if b.id != e.block {
            return None;
        }
        let bytes = b.data.get(e.off as usize..(e.off + e.len) as usize)?;
        let (reply, rest) = bytes.split_at(e.reply_len as usize);
        let (headers, payload) = rest.split_at(e.hdr_len as usize);
        Some(View {
            entry: *e,
            subject: &self.subjects[id as usize].name,
            reply: (!reply.is_empty()).then(|| std::str::from_utf8(reply).unwrap_or("")),
            headers: (e.hdr_len > 0).then_some(headers),
            payload,
            time_ms: self.epoch_ms + e.time as u64,
        })
    }

    /// Subject `id`'s message `seq`.
    pub fn get(&self, id: u32, seq: u32) -> Option<View<'_>> {
        let s = self.subjects.get(id as usize)?;
        let e = s.msgs.get(s.position(seq)?)?;
        self.view(id, e)
    }

    /// Subject `id`'s `i`th message, newest first.
    pub fn newest(&self, id: u32, i: usize) -> Option<View<'_>> {
        let s = self.subjects.get(id as usize)?;
        let e = s.msgs.get(s.msgs.len().checked_sub(i + 1)?)?;
        self.view(id, e)
    }

    pub fn epoch_ms(&self) -> u64 {
        self.epoch_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(store: &mut Store, conn: u16, subject: &str, payload: &[u8], now: u64) -> Option<u32> {
        let id = store.subject(conn, subject, now);
        store.count(id, payload.len(), now).then(|| store.store(id, None, None, 0, payload, now))
    }

    #[test]
    fn messages_are_counted_and_kept() {
        let mut s = Store::new(Limits::default(), 1_000_000);
        put(&mut s, 0, "a", b"one", 1_000_100);
        put(&mut s, 0, "a", b"two!", 1_000_200);
        put(&mut s, 1, "a", b"other connection", 1_000_300);
        let a = s.find(0, "a").unwrap();
        assert_ne!(Some(a), s.find(1, "a"));
        let sub = &s.subjects[a as usize];
        assert_eq!((sub.count, sub.bytes, sub.msgs.len()), (2, 7, 2));
        assert_eq!(s.newest(a, 0).unwrap().payload, b"two!");
        assert_eq!(s.newest(a, 1).unwrap().payload, b"one");
        assert_eq!(s.newest(a, 1).unwrap().time_ms, 1_000_100);
        assert!(s.newest(a, 2).is_none());
        assert_eq!(s.totals.msgs, 3);
    }

    #[test]
    fn reply_and_headers_come_back() {
        let mut s = Store::new(Limits::default(), 0);
        let id = s.subject(0, "svc", 0);
        s.count(id, 20, 0);
        let seq = s.store(id, Some("_INBOX.x"), Some(b"NATS/1.0\r\nA: 1\r\n\r\n"), 1, b"hi", 0);
        let v = s.get(id, seq).unwrap();
        assert_eq!((v.reply, v.headers.map(|h| h.len()), v.payload, v.entry.header_count), (Some("_INBOX.x"), Some(18), b"hi".as_slice(), 1));
    }

    #[test]
    fn a_subject_keeps_its_newest_within_its_caps() {
        let mut s = Store::new(Limits { per_subject: 3, ..Limits::default() }, 0);
        for i in 0..10u8 {
            put(&mut s, 0, "a", &[i], 0);
        }
        let a = s.find(0, "a").unwrap();
        assert_eq!(s.subjects[a as usize].msgs.len(), 3);
        assert_eq!(s.newest(a, 2).unwrap().payload, [7]);
        assert_eq!(s.subjects[a as usize].evicted, 7);
        let mut s = Store::new(Limits { per_subject_bytes: 100, ..Limits::default() }, 0);
        for _ in 0..10 {
            put(&mut s, 0, "a", &[0; 30], 0);
        }
        assert_eq!(s.subjects[0].msgs.len(), 3);
        assert!(s.subjects[0].stored_bytes <= 100);
    }

    #[test]
    fn the_budget_lets_the_oldest_go_whatever_their_subject() {
        let mut s = Store::new(Limits { budget: 2 * BLOCK, per_subject: 1_000_000, per_subject_bytes: usize::MAX, max_subjects: 100 }, 0);
        let big = vec![7u8; 100_000];
        for i in 0..100 {
            put(&mut s, 0, if i % 2 == 0 { "even" } else { "odd" }, &big, i);
        }
        assert!(s.blocks.len() <= 2);
        let kept: usize = s.subjects.iter().map(|x| x.msgs.len()).sum();
        assert!((10..=20).contains(&kept), "{kept}");
        // What's kept is the newest, and each still reads.
        let even = s.find(0, "even").unwrap();
        assert_eq!(s.newest(even, 0).unwrap().time_ms, 98);
        for x in 0..s.subjects.len() as u32 {
            for i in 0..s.subjects[x as usize].msgs.len() {
                assert_eq!(s.newest(x, i).unwrap().payload.len(), 100_000);
            }
        }
        assert_eq!(s.totals.evicted as usize + kept, 100);
        assert!(s.memory() < 4 * BLOCK);
    }

    #[test]
    fn entries_are_capped_too() {
        let mut s = Store::new(Limits { budget: 4 * BLOCK, per_subject: 1_000_000, per_subject_bytes: usize::MAX, max_subjects: 10 }, 0);
        let max = s.limits.max_entries();
        for i in 0..(max * 3) {
            put(&mut s, 0, ["a", "b", "c"][i % 3], b"x", 0);
        }
        assert!(s.entries() <= max);
        assert!(s.fifo.len() <= max);
    }

    #[test]
    fn a_huge_message_is_kept_cut_short() {
        let mut s = Store::new(Limits::default(), 0);
        let id = s.subject(0, "big", 0);
        s.count(id, 3 * BLOCK, 0);
        let seq = s.store(id, None, None, 0, &vec![1u8; 3 * BLOCK], 0);
        let v = s.get(id, seq).unwrap();
        assert!(v.entry.truncated);
        assert_eq!(v.payload.len(), BLOCK);
        assert_eq!(v.entry.size as usize, 3 * BLOCK);
    }

    #[test]
    fn sampling_keeps_one_in_n_of_a_busy_subject_but_all_of_a_quiet_one() {
        let mut s = Store::new(Limits { per_subject: 1_000_000, ..Limits::default() }, 0);
        s.sampling = 10;
        for i in 0..1000 {
            put(&mut s, 0, "busy", b"x", 5000 + i / 100);
        }
        for i in 0..10 {
            put(&mut s, 0, "quiet", b"x", 5000 + i);
        }
        let busy = &s.subjects[s.find(0, "busy").unwrap() as usize];
        assert_eq!(busy.count, 1000);
        assert!(busy.msgs.len() < 150, "{}", busy.msgs.len());
        assert_eq!(busy.skipped as usize + busy.msgs.len(), 1000);
        assert_eq!(s.subjects[s.find(0, "quiet").unwrap() as usize].msgs.len(), 10);
        s.paused = true;
        assert!(put(&mut s, 0, "quiet", b"x", 6000).is_none());
    }

    #[test]
    fn subjects_past_the_cap_are_counted_together() {
        let mut s = Store::new(Limits { max_subjects: 3, ..Limits::default() }, 0);
        for i in 0..10 {
            put(&mut s, 0, &format!("s.{i}"), b"x", 0);
        }
        assert_eq!(s.subjects.len(), 4);
        assert!(s.subjects[3].overflow);
        assert_eq!(s.subjects[3].count, 7);
    }

    #[test]
    fn the_rate_rolls_by_the_second() {
        let mut r = Rate::default();
        for _ in 0..10 {
            r.hit(100, 1);
        }
        r.hit(101, 4);
        r.hit(103, 2);
        assert_eq!(r.current(103), 2);
        assert_eq!(r.per_sec(104), (4.0 + 0.0 + 2.0) / 3.0);
        assert_eq!(r.series(104, 4), vec![10, 4, 0, 2]);
        assert_eq!(r.per_sec(200), 0.0, "a minute later, nothing");
        r.hit(500, 1);
        assert_eq!(r.series(501, 3), vec![0, 0, 1]);
    }

    #[test]
    fn positions_are_found_by_seq() {
        let mut s = Store::new(Limits::default(), 0);
        let mut seqs = Vec::new();
        for i in 0..20u8 {
            put(&mut s, 0, "x", &[i], 0);
            seqs.push(put(&mut s, 0, "a", &[i], 0).unwrap());
        }
        let a = s.find(0, "a").unwrap();
        for (i, seq) in seqs.iter().enumerate() {
            assert_eq!(s.subjects[a as usize].position(*seq), Some(i));
            assert_eq!(s.get(a, *seq).unwrap().payload, [i as u8]);
        }
        assert_eq!(s.subjects[a as usize].position(seqs[3] + 1), None);
    }
}
