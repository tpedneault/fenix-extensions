//! How much memory a large MIB takes once read: writes a synthetic
//! mission-sized MIB (or reads the folder given), loads it as the
//! extensions do, and prints the heap at its peak and once loaded.
//!
//! `cargo run --release --example footprint [-- <mib folder>]`

use std::alloc::{GlobalAlloc, Layout, System};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let n = NOW.fetch_add(l.size(), Ordering::Relaxed) + l.size();
            PEAK.fetch_max(n, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        NOW.fetch_sub(l.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static A: Counting = Counting;

fn mb(n: usize) -> String {
    format!("{:.1} MiB", n as f64 / (1 << 20) as f64)
}

/// A mission-sized MIB: 10k telecommands of 10 elements, 20k TC and
/// 60k TM parameters, 5k packets of 30 parameters, and their
/// calibrations.
fn write_big(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).unwrap();
    let mut t: std::collections::HashMap<&str, String> = Default::default();
    let mut put = |table: &'static str, line: String| {
        let s = t.entry(table).or_default();
        s.push_str(&line);
        s.push('\n');
    };
    for i in 0..10_000 {
        put("ccf", format!("ZTC{i:05}\tTelecommand number {i} doing something useful\t\tA\tN\t\t8\t1\t{}\t10\t\t\t\t\tSUBSYS\t\t\t\t\t\t9", 100 + i % 50));
        for k in 0..10 {
            put("cdf", format!("ZTC{i:05}\tE\tArgument {k} of telecommand {i}\t16\t{}\t0\tPTC{:05}\t\t\t", k * 16, (i * 2 + k) % 20_000));
        }
    }
    for p in 0..20_000 {
        put("cpc", format!("PTC{p:05}\tTC parameter {p} description\t3\t12\t\tD\tV\t\tPRF{:05}\t\tPAF{:05}\t\t1\t\t\t\t", p % 2000, p % 3000));
    }
    for c in 0..3000 {
        put("paf", format!("PAF{c:05}\tStatus {c}\tU\t4"));
        for v in 0..4 {
            put("pas", format!("PAF{c:05}\tSTATE_{v}\t{v}"));
        }
    }
    for c in 0..2000 {
        put("prf", format!("PRF{c:05}\tRange {c}\tR\tI\tD\t1\tV"));
        put("prv", format!("PRF{c:05}\t0\t4095"));
    }
    for p in 0..60_000 {
        put("pcf", format!("NTM{p:05}\tTM parameter {p} with a longer description\t{p}\tdegC\t3\t12\t\t\t\tN\tR\tCAF{:05}\t\t\t\t\tSUBSYS\t\t\t\t\t\t\t", p % 5000));
        put("ocp", format!("NTM{p:05}\t1\tS\t-10\t50\t\t"));
    }
    for c in 0..5000 {
        put("caf", format!("CAF{c:05}\tCurve {c}\tR\tU\tD\tdegC\t8\tP"));
        for k in 0..8 {
            put("cap", format!("CAF{c:05}\t{}\t{}", k * 500, k * 10));
        }
    }
    for s in 0..5000 {
        put("pid", format!("3\t25\t{}\t{s}\t0\t{}\tHousekeeping packet {s}\t\t-1\t13\tY\t\t\t1\t\t", 100 + s % 50, 30_000 + s));
        put("tpcf", format!("{}\tHK_{s:05}\t200", 30_000 + s));
        for k in 0..30 {
            put("plf", format!("NTM{:05}\t{}\t{}\t0\t\t\t\t", (s * 30 + k) % 60_000, 30_000 + s, 20 + k * 2));
        }
    }
    for (table, text) in t {
        std::fs::write(dir.join(format!("{table}.dat")), text).unwrap();
    }
}

fn main() {
    let dir = match std::env::args().nth(1) {
        Some(d) => std::path::PathBuf::from(d),
        None => {
            let d = std::env::temp_dir().join("fenix-mib-footprint");
            if !d.join("ccf.dat").exists() {
                write_big(&d);
            }
            d
        }
    };
    let on_disk: u64 = std::fs::read_dir(&dir).unwrap().flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum();
    let base = NOW.load(Ordering::Relaxed);
    {
        let mut index = fenix_mib::MibIndex::new(vec![fenix_mib::MibRoot { label: "BIG".into(), path: dir.clone() }]);
        index.refresh();
        println!("the tables alone: {}", mb(NOW.load(Ordering::Relaxed) - base));
    }
    let base = NOW.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let t = std::time::Instant::now();
    let set = fenix_mib::MibSet::load(vec![fenix_mib::MibRoot { label: "BIG".into(), path: dir.clone() }], None);
    let took = t.elapsed();
    let mut out = String::new();
    let rows: usize = set.index().options().schema.tables().iter().map(|t| set.index().rows(t).len()).sum();
    writeln!(out, "{} on disk, {rows} rows, read in {took:.2?}", mb(on_disk as usize)).unwrap();
    writeln!(out, "loaded: {}   peak while loading: {}", mb(NOW.load(Ordering::Relaxed) - base), mb(PEAK.load(Ordering::Relaxed) - base)).unwrap();
    print!("{out}");
    drop(set);
}
