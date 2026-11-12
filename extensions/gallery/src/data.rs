//! The sample data the sections show: made up here, the same every time,
//! and nobody's. Places are real; the weather, the project, its build and
//! its tasks are invented.

/// A day's weather at a station.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub station: &'static str,
    pub date: String,
    pub high: f64,
    pub low: f64,
    pub rain: f64,
    pub wind: u32,
    pub conditions: &'static str,
}

/// Stations: a name, a typical high and low in March, how wet, how windy.
const STATIONS: [(&str, f64, f64, f64, u32); 20] = [
    ("Tromsø", 1.0, -5.0, 2.5, 22),
    ("Reykjavík", 4.0, -1.0, 3.0, 30),
    ("Hobart", 20.0, 11.0, 1.5, 18),
    ("札幌 Sapporo", 4.0, -4.0, 2.0, 14),
    ("Ushuaia", 11.0, 4.0, 1.8, 26),
    ("Valparaíso", 22.0, 13.0, 0.3, 16),
    ("Marrakesh", 24.0, 10.0, 0.6, 10),
    ("Tórshavn", 6.0, 1.0, 4.0, 34),
    ("Darwin", 32.0, 24.0, 9.0, 12),
    ("Halifax", 3.0, -6.0, 3.2, 24),
    ("Kraków", 9.0, -1.0, 1.2, 12),
    ("Nairobi", 26.0, 13.0, 2.4, 11),
    ("Wellington", 20.0, 13.0, 2.0, 32),
    ("Yellowknife", -12.0, -25.0, 0.4, 15),
    ("Lisbon", 18.0, 10.0, 1.6, 17),
    ("Ulaanbaatar", -2.0, -17.0, 0.1, 13),
    ("Chennai", 32.0, 23.0, 0.4, 14),
    ("Punta Arenas", 13.0, 5.0, 1.4, 38),
    ("Kyiv", 7.0, -1.0, 1.3, 13),
    ("Svalbard", -9.0, -17.0, 0.6, 20),
];

const CONDITIONS: [&str; 12] = [
    "Clear and still",
    "Bright but cold, a sharp frost at dawn",
    "Low cloud clearing by midday, a light breeze off the water",
    "Showers on and off, heavier in the afternoon with a few rumbles of thunder",
    "Fog in the harbour until mid-morning, then hazy sunshine",
    "Drizzle",
    "Snow flurries overnight, settling on high ground above 400 m",
    "Gale from the south-west, gusting 90 km/h on exposed coasts",
    "Warm and humid, a sea breeze in the afternoon",
    "Overcast",
    "Sunny spells",
    "Heavy rain from dawn, easing in the evening",
];

/// A small generator, so the data is the same every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    /// From -1.0 to 1.0.
    fn unit(&mut self) -> f64 {
        (self.next() % 2001) as f64 / 1000.0 - 1.0
    }
}

fn tenths(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// Twenty days of March at twenty stations: 400 rows.
pub fn observations() -> Vec<Observation> {
    let mut rng = Lcg(2026);
    let mut out = Vec::with_capacity(400);
    for day in 1..=20 {
        for &(station, high, low, wet, wind) in &STATIONS {
            let swing = rng.unit() * 4.0;
            let rain = (wet * (1.0 + rng.unit() * 1.5)).max(0.0);
            let rain = if rng.next().is_multiple_of(3) { 0.0 } else { tenths(rain * 2.0) };
            let cold = low + swing < 0.0;
            let pick = (rng.next() as usize) % CONDITIONS.len();
            let conditions = match (rain > 0.0, cold) {
                (true, true) => CONDITIONS[6],
                (true, false) if pick.is_multiple_of(2) => CONDITIONS[3],
                (true, false) => CONDITIONS[[5, 11, 7][pick % 3]],
                (false, true) => CONDITIONS[[0, 1, 9][pick % 3]],
                (false, false) => CONDITIONS[[0, 2, 4, 8, 9, 10][pick % 6]],
            };
            out.push(Observation {
                station,
                date: format!("2026-03-{day:02}"),
                high: tenths(high + swing + rng.unit()),
                low: tenths(low + swing + rng.unit()),
                rain,
                wind: (wind as f64 * (1.0 + rng.unit() * 0.6)).round() as u32,
                conditions,
            });
        }
    }
    out
}

/// The first 36 elements: name, symbol, number.
pub const ELEMENTS: [(&str, &str, u32); 36] = [
    ("Hydrogen", "H", 1),
    ("Helium", "He", 2),
    ("Lithium", "Li", 3),
    ("Beryllium", "Be", 4),
    ("Boron", "B", 5),
    ("Carbon", "C", 6),
    ("Nitrogen", "N", 7),
    ("Oxygen", "O", 8),
    ("Fluorine", "F", 9),
    ("Neon", "Ne", 10),
    ("Sodium", "Na", 11),
    ("Magnesium", "Mg", 12),
    ("Aluminium", "Al", 13),
    ("Silicon", "Si", 14),
    ("Phosphorus", "P", 15),
    ("Sulfur", "S", 16),
    ("Chlorine", "Cl", 17),
    ("Argon", "Ar", 18),
    ("Potassium", "K", 19),
    ("Calcium", "Ca", 20),
    ("Scandium", "Sc", 21),
    ("Titanium", "Ti", 22),
    ("Vanadium", "V", 23),
    ("Chromium", "Cr", 24),
    ("Manganese", "Mn", 25),
    ("Iron", "Fe", 26),
    ("Cobalt", "Co", 27),
    ("Nickel", "Ni", 28),
    ("Copper", "Cu", 29),
    ("Zinc", "Zn", 30),
    ("Gallium", "Ga", 31),
    ("Germanium", "Ge", 32),
    ("Arsenic", "As", 33),
    ("Selenium", "Se", 34),
    ("Bromine", "Br", 35),
    ("Krypton", "Kr", 36),
];

/// A release checklist: what, when, and whether it's done.
pub const TASKS: [(&str, &str, bool); 12] = [
    ("Bump the version in Cargo.toml and extension.toml", "Mon 16", true),
    ("Write the CHANGELOG entry", "Mon 16", true),
    ("Run cargo xtask validate", "Tue 17", true),
    ("Fix the clippy warning in tides/harmonics.rs", "Tue 17", false),
    ("Retake the screenshots", "Wed 18", false),
    ("Check the page at 80 columns", "Wed 18", false),
    ("Try it on standard keys", "Wed 18", false),
    ("Review the permissions comment", "Thu 19", false),
    ("Pack it and note the size", "Thu 19", false),
    ("Install the package in a clean profile", "Thu 19", false),
    ("Tag harbour-tides-v0.5.0", "Fri 20", false),
    ("Tell the mailing list", "Fri 20", false),
];

/// The files of a made-up project, harbour-tides: path, size in bytes.
pub const FILES: [(&str, u32); 14] = [
    ("Cargo.toml", 612),
    ("README.md", 3_870),
    ("CHANGELOG.md", 1_204),
    ("src/main.rs", 2_150),
    ("src/cli.rs", 4_431),
    ("src/tides/mod.rs", 905),
    ("src/tides/harmonics.rs", 14_812),
    ("src/tides/stations.rs", 7_266),
    ("src/tides/predict.rs", 9_940),
    ("tests/harmonics.rs", 5_388),
    ("tests/predict.rs", 3_019),
    ("docs/constituents.md", 11_502),
    ("docs/datums.md", 2_675),
    ("benches/year.rs", 1_833),
];

/// Fixtures the tree loads only when their folder is opened.
pub const FIXTURES: [(&str, u32); 4] = [("tests/fixtures/brest-2025.csv", 88_214), ("tests/fixtures/dover-2025.csv", 87_960), ("tests/fixtures/constituents.toml", 3_302), ("tests/fixtures/README.md", 410)];

/// `n` bytes, as a file list shows them.
pub fn size(n: u32) -> String {
    if n < 1024 {
        format!("{n} B")
    } else {
        format!("{:.1} KB", n as f64 / 1024.0)
    }
}

/// A build's output, 300 lines: crates compiled, a few warnings, an
/// error and its fix, then the build again.
pub fn build_log() -> Vec<String> {
    const CRATES: [(&str, &str); 24] = [
        ("proc-macro2", "1.0.95"),
        ("unicode-ident", "1.0.18"),
        ("libc", "0.2.172"),
        ("cfg-if", "1.0.0"),
        ("autocfg", "1.4.0"),
        ("serde", "1.0.219"),
        ("quote", "1.0.40"),
        ("syn", "2.0.101"),
        ("memchr", "2.7.4"),
        ("itoa", "1.0.15"),
        ("ryu", "1.0.20"),
        ("serde_json", "1.0.140"),
        ("num-traits", "0.2.19"),
        ("chrono", "0.4.41"),
        ("clap_lex", "0.7.4"),
        ("anstyle", "1.0.10"),
        ("clap_builder", "4.5.38"),
        ("clap", "4.5.38"),
        ("csv-core", "0.1.12"),
        ("csv", "1.3.1"),
        ("toml_datetime", "0.6.9"),
        ("toml", "0.8.22"),
        ("thiserror", "2.0.12"),
        ("harbour-tides", "0.5.0"),
    ];
    let mut out = Vec::new();
    let mut second = 0u32;
    let mut stamp = |out: &mut Vec<String>, text: String| {
        out.push(format!("[09:{:02}:{:02}] {text}", 14 + second / 60, second % 60));
        second += 1;
    };
    for round in 0..6 {
        stamp(&mut out, format!("cargo build --release   (run {} of 6)", round + 1));
        for (i, (name, version)) in CRATES.iter().enumerate() {
            stamp(&mut out, format!("   Compiling {name} v{version}"));
            if round == 2 && i == 23 {
                stamp(&mut out, "warning: unused variable: `phase` --> src/tides/harmonics.rs:88:9".into());
                stamp(&mut out, "warning: field `datum` is never read --> src/tides/stations.rs:41:5".into());
                stamp(&mut out, "error[E0308]: mismatched types --> src/tides/predict.rs:132:24: expected `f64`, found `f32`".into());
                stamp(&mut out, "error: could not compile `harbour-tides` (bin) due to 1 previous error; 2 warnings emitted".into());
            }
        }
        if round != 2 {
            stamp(&mut out, format!("    Finished `release` profile [optimized] target(s) in {}.{}s", 38 + round * 3, round * 7 % 10));
        }
        stamp(&mut out, String::new());
    }
    while out.len() < 300 {
        let n = out.len();
        stamp(&mut out, format!("     Running tests/predict.rs: high water at Brest, day {} ... ok", n - 155));
    }
    out.truncate(300);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_is_the_same_every_time() {
        let a = observations();
        assert_eq!(a.len(), 400);
        assert_eq!(a, observations());
        assert!(a.iter().any(|o| o.low < 0.0) && a.iter().any(|o| o.high >= 30.0), "frost and heat both, to colour");
        assert_eq!(build_log().len(), 300);
        assert!(build_log().iter().any(|l| l.contains("error[")));
        assert_eq!(size(612), "612 B");
        assert_eq!(size(14_812), "14.5 KB");
    }
}
