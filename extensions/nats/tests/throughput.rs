//! The ingest path at speed: synthetic `MSG` streams fed as the host feeds
//! a socket, in 64 KiB chunks, through the parser, the counters, the store
//! and the rules -- what each chunk costs, and how many messages a second
//! that makes.
//!
//! ```text
//! cargo test --release --test throughput -- --ignored --nocapture
//! ```

use std::time::Instant;

use fenix_nats_client::conn::{Addr, Auth, Net};
use fenix_nats_client::core::Core;
use fenix_nats_client::filter::parse_rules;
use fenix_nats_client::store::Limits;

#[derive(Default)]
struct Null {
    next: u64,
}

impl Net for Null {
    fn connect(&mut self, _: &str, _: u16, _: bool) -> Result<u64, String> {
        self.next += 1;
        Ok(self.next)
    }
    fn start_tls(&mut self, _: u64, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn write(&mut self, _: u64, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn close(&mut self, _: u64) {}
}

/// `n` messages, `min`..`max` bytes of payload, over `subjects` subjects;
/// one in eight with headers, one in four JSON.
fn stream(n: usize, min: usize, max: usize, subjects: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(n * (max + min) / 2 + n * 40);
    let mut x: u64 = 0x2545_f491_4f6c_dd1d;
    let filler: Vec<u8> = (0..max).map(|i| b'a' + (i % 26) as u8).collect();
    for i in 0..n {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let size = min + (x as usize % (max - min + 1));
        let subject = format!("telemetry.site{}.sensor{}", (x >> 20) as usize % 10, (x >> 32) as usize % (subjects / 10).max(1));
        let payload: Vec<u8> = if i % 4 == 0 {
            let mut p = format!("{{\"seq\":{i},\"ok\":{},\"v\":\"", i % 100 != 0).into_bytes();
            p.extend_from_slice(&filler[..size.saturating_sub(p.len() + 2)]);
            p.extend_from_slice(b"\"}");
            p
        } else {
            filler[..size].to_vec()
        };
        if i % 8 == 0 {
            let h = b"NATS/1.0\r\nNats-Msg-Id: x\r\nSource: bench\r\n\r\n";
            out.extend_from_slice(format!("HMSG {subject} 1 {} {}\r\n", h.len(), h.len() + payload.len()).as_bytes());
            out.extend_from_slice(h);
        } else {
            out.extend_from_slice(format!("MSG {subject} 1 {}\r\n", payload.len()).as_bytes());
        }
        out.extend_from_slice(&payload);
        out.extend_from_slice(b"\r\n");
    }
    out
}

fn connected(limits: Limits) -> (Core, Null, u64) {
    let mut net = Null::default();
    let mut core = Core::new(limits, 1_700_000_000_000_000);
    core.set_servers(&mut net, vec![Addr::parse("localhost", "SPC o m a").unwrap()], &[">".into()]);
    core.connect(&mut net, 0, Auth::None, 1_700_000_000_000_000);
    let s = core.conns[0].socket.unwrap();
    core.on_connected(&mut net, s, Ok(()), 1_700_000_000_000_000);
    core.on_data(&mut net, s, b"INFO {\"headers\":true}\r\nPONG\r\n", 1_700_000_000_000_000);
    (core, net, s)
}

struct Run {
    msgs: u64,
    secs: f64,
    max_chunk_us: u64,
    p99_us: u64,
    slowest_at: usize,
    over_1ms: usize,
    chunks: usize,
    sampling: u32,
    stored: u64,
}

/// Feeds `wire` in 64 KiB chunks, the clock moving as fast as the work does
/// (the timer's ticks every 100 ms of it), as the extension would run.
fn feed(core: &mut Core, net: &mut Null, s: u64, wire: &[u8]) -> Run {
    let start = Instant::now();
    let base = 1_700_000_000_000_000u64;
    let mut last_tick = 0u64;
    let mut max_chunk = 0u64;
    let mut times = Vec::new();
    for chunk in wire.chunks(64 * 1024) {
        let t = Instant::now();
        let now = base + start.elapsed().as_micros() as u64;
        core.on_data(net, s, chunk, now);
        let spent = t.elapsed().as_micros() as u64;
        core.spent(now, spent);
        max_chunk = max_chunk.max(spent);
        times.push(spent);
        let elapsed = now - base;
        if elapsed - last_tick >= 100_000 {
            let late = (elapsed - last_tick).saturating_sub(100_000);
            core.tick(net, now, late, &mut |_| Auth::None);
            core.events.clear();
            last_tick = elapsed;
        }
    }
    let slowest_at = times.iter().position(|t| *t == max_chunk).unwrap_or(0);
    let mut sorted = times.clone();
    sorted.sort_unstable();
    let p99_us = sorted.get(sorted.len() * 99 / 100).copied().unwrap_or(0);
    let over_1ms = times.iter().filter(|t| **t > 1000).count();
    Run { msgs: core.store.totals.msgs, secs: start.elapsed().as_secs_f64(), max_chunk_us: max_chunk, p99_us, slowest_at, over_1ms, chunks: times.len(), sampling: core.store.sampling, stored: core.store.totals.stored }
}

#[test]
fn every_message_is_counted_whatever_the_chunks() {
    let wire = stream(50_000, 16, 1024, 200);
    let (mut core, mut net, s) = connected(Limits { budget: 8 << 20, ..Limits::default() });
    let run = feed(&mut core, &mut net, s, &wire);
    assert_eq!(run.msgs, 50_000);
    let counted: u64 = core.store.subjects.iter().map(|x| x.count).sum();
    assert_eq!(counted, 50_000);
    assert!(core.store.memory() < 16 << 20, "{}", core.store.memory());
    assert_eq!(core.store.totals.stored + core.store.totals.skipped, 50_000);
}

#[test]
#[ignore = "a benchmark: run it with --release --ignored --nocapture"]
fn throughput() {
    for (label, n, min, max, rules) in [
        ("1M messages of 16 B - 1 KiB", 1_000_000usize, 16usize, 1024usize, ""),
        ("1M small messages, 16 - 64 B", 1_000_000, 16, 64, ""),
        ("1M of 16 B - 1 KiB, a notifying rule with a regex", 1_000_000, 16, 1024, "!bad telemetry.site1.* re:\"ok\":false"),
    ] {
        let wire = stream(n, min, max, 500);
        let (mut core, mut net, s) = connected(Limits::default());
        if !rules.is_empty() {
            core.rules.set(parse_rules(rules).unwrap());
        }
        let run = feed(&mut core, &mut net, s, &wire);
        assert_eq!(run.msgs, n as u64);
        println!(
            "{label}: {:.0} msg/s, {:.0} MiB/s, longest chunk {} µs (chunk {} of {}), 99% under {} µs, {} over 1 ms, {} subjects, {} stored ({} kept now), sampling 1:{}, store {:.1} MiB",
            run.msgs as f64 / run.secs,
            wire.len() as f64 / run.secs / 1048576.0,
            run.max_chunk_us,
            run.slowest_at,
            run.chunks,
            run.p99_us,
            run.over_1ms,
            core.store.subjects.len(),
            run.stored,
            core.store.entries(),
            run.sampling,
            core.store.memory() as f64 / 1048576.0
        );
    }
}
