//! A big log, timed: written to a folder (about 200 MB unless told
//! otherwise), then indexed and filtered the way the page does it, a step
//! at a time -- how long each step takes, and how much memory it all
//! holds.
//!
//! ```text
//! cargo run --release --example big -- /tmp/logs 200
//! ```

use std::io::Write;
use std::rc::Rc;
use std::time::{Duration, Instant};

use fenix_logs::detect::Rules;
use fenix_logs::filter::{Filter, Matcher, Pass};
use fenix_logs::index::{Index, STEP};
use fenix_logs::page::LogPage;
use fenix_logs::source::Source;
use fenix_ui_kit::Key;

const LEVELS: [&str; 6] = ["INFO", "INFO", "INFO", "DEBUG", "WARN", "ERROR"];

/// The most memory the process has held, from /proc.
fn peak_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmHWM:")).and_then(|l| l.split_whitespace().nth(1)?.parse().ok()))
        .unwrap_or(0)
}

fn write(path: &std::path::Path, mib: u64) {
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut seed = 12345u64;
    let mut written = 0u64;
    let mut i = 0u64;
    while written < mib * 1024 * 1024 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let level = LEVELS[(seed >> 33) as usize % LEVELS.len()];
        let s = i / 40;
        let line = format!(
            "2024-03-{:02}T{:02}:{:02}:{:02}.{:03}Z {level:<5} worker-{} request id={} user=u{} took {} ms\n",
            1 + s / 86_400,
            s / 3600 % 24,
            s / 60 % 60,
            s % 60,
            i % 40 * 25,
            seed % 8,
            i,
            seed % 997,
            seed % 5000
        );
        written += line.len() as u64;
        f.write_all(line.as_bytes()).unwrap();
        if level == "ERROR" && seed.is_multiple_of(3) {
            let trace = "java.lang.RuntimeException: boom\n\tat com.acme.Worker.run(Worker.java:42)\n\tat java.lang.Thread.run(Thread.java:840)\n";
            written += trace.len() as u64;
            f.write_all(trace.as_bytes()).unwrap();
        }
        i += 1;
    }
}

/// Steps until done, timed: how many, the longest, the mean.
fn steps(what: &str, mut step: impl FnMut() -> bool) {
    let mut times = Vec::new();
    let all = Instant::now();
    loop {
        let t = Instant::now();
        let more = step();
        times.push(t.elapsed());
        if !more {
            break;
        }
    }
    let max = times.iter().max().copied().unwrap_or_default();
    let mean = times.iter().sum::<Duration>() / times.len().max(1) as u32;
    println!(
        "{what:<44} {:>5} steps · longest {:>6.1} ms · mean {:>5.1} ms · all {:>7.1} ms",
        times.len(),
        max.as_secs_f64() * 1000.0,
        mean.as_secs_f64() * 1000.0,
        all.elapsed().as_secs_f64() * 1000.0
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().unwrap_or_else(|| std::env::temp_dir().join("fenix-logs-big").to_string_lossy().into_owned()));
    let mib: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(200);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.log");
    if std::fs::metadata(&path).map(|m| m.len() < mib * 1024 * 1024).unwrap_or(true) {
        let t = Instant::now();
        write(&path, mib);
        println!("wrote {} in {:.1} s", path.display(), t.elapsed().as_secs_f64());
    }
    let before = peak_kib();
    let src = Source::path(path.to_str().unwrap()).unwrap();
    println!("{:.1} MiB; the process held {} KiB before", src.len() as f64 / (1024.0 * 1024.0), before);

    let rules = Rc::new(Rules { custom: Vec::new(), year: 2024 });
    let mut index = Index::new(rules.clone(), src.len());
    steps(&format!("index, {} KiB a step", STEP >> 10), || index.step(&src, STEP));
    println!("  {} entries, {} lines, {} chunks; the index holds about {} KiB", index.entries(), index.lines(), index.chunk_count(), index.memory() / 1024);

    let errors = Filter { levels: [true, false, false, false, false, false], ..Filter::default() };
    let mut pass = Pass::new(errors, None, &index);
    steps("filter: errors only (levels, nothing read)", || pass.step(&index, &src, STEP));
    println!("  {} shown", pass.rows(&index));

    let text = Filter { text: Some(Matcher::new("user=u42 ").unwrap()), ..Filter::default() };
    let mut pass = Pass::new(text, Some(Matcher::new("re:took 49\\d\\d ms").unwrap()), &index);
    steps(&format!("filter: a text and a search, {} KiB a step", STEP >> 10), || pass.step(&index, &src, STEP));
    println!("  {} shown, {} found; the filter holds about {} KiB", pass.rows(&index), pass.found(), pass.memory() / 1024);

    let mut page = LogPage::new(path.to_str().unwrap(), path.to_str().unwrap(), Source::path(path.to_str().unwrap()), rules);
    steps("the page's calls: index and filter, 8 ms each", || page.work());
    let t = Instant::now();
    page.key(Key::Char('G'));
    let c = page.render(160, 50);
    println!("{:<44} {:>6.1} ms", "G, and the last screen drawn", t.elapsed().as_secs_f64() * 1000.0);
    let t = Instant::now();
    page.key(Key::Char('['));
    page.key(Key::Enter);
    page.render(160, 50);
    println!("{:<44} {:>6.1} ms", "[ to the error before, opened, drawn", t.elapsed().as_secs_f64() * 1000.0);
    println!("\n{}\n", (0..12).map(|y| c.line(y)).collect::<Vec<_>>().join("\n"));
    println!(
        "the process held at most {} KiB ({:.1} MiB) -- {} KiB more than before",
        peak_kib(),
        peak_kib() as f64 / 1024.0,
        peak_kib().saturating_sub(before)
    );
}
