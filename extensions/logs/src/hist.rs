//! Entries counted by time, in a fixed number of buckets that widen as
//! the times seen spread out -- so a log of any length is counted in the
//! same little memory -- and drawn as a row of bars.

use crate::detect::Level;

/// How many buckets there are.
const BUCKETS: usize = 512;
/// The narrowest a bucket is: a millisecond.
const NARROWEST: i64 = 1;

/// A bucket's counts: all its entries, its warnings, its errors.
pub type Counts = [u32; 3];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Histogram {
    start: i64,
    width: i64,
    buckets: Vec<Counts>,
    /// The first and last time seen.
    pub range: Option<(i64, i64)>,
}

impl Histogram {
    pub fn add(&mut self, t: i64, level: Level) {
        if self.buckets.is_empty() {
            self.width = NARROWEST;
            self.start = t.div_euclid(NARROWEST) * NARROWEST;
            self.buckets = vec![[0; 3]; BUCKETS];
        }
        while t < self.start {
            self.widen(true);
        }
        while t >= self.start + self.width * BUCKETS as i64 {
            self.widen(false);
        }
        let b = &mut self.buckets[((t - self.start) / self.width) as usize];
        b[0] += 1;
        if level == Level::Warn {
            b[1] += 1;
        }
        if level.is_error() {
            b[2] += 1;
        }
        self.range = Some(match self.range {
            Some((a, z)) => (a.min(t), z.max(t)),
            None => (t, t),
        });
    }

    /// Twice as wide: what was there in the upper half when `back` (to
    /// take earlier times), else in the lower.
    fn widen(&mut self, back: bool) {
        let mut wide = vec![[0u32; 3]; BUCKETS];
        let shift = if back { BUCKETS / 2 } else { 0 };
        for (i, b) in self.buckets.iter().enumerate() {
            let w = &mut wide[shift + i / 2];
            for k in 0..3 {
                w[k] += b[k];
            }
        }
        if back {
            self.start -= self.width * BUCKETS as i64;
        }
        self.width *= 2;
        self.buckets = wide;
    }

    /// The counts in `cols` columns spread over `from..=to`: a bucket
    /// across two columns shared between them, so the bars don't step
    /// where buckets and columns don't line up.
    pub fn columns(&self, from: i64, to: i64, cols: usize) -> Vec<Counts> {
        let mut out = vec![[0f64; 3]; cols];
        if cols == 0 || self.buckets.is_empty() {
            return vec![[0; 3]; cols];
        }
        let span = (to - from).max(1) as f64;
        let col = span / cols as f64;
        for (i, b) in self.buckets.iter().enumerate() {
            if b[0] == 0 {
                continue;
            }
            // The bucket's span, in columns, within the range.
            let start = self.start + i as i64 * self.width;
            let a = ((start.max(from) - from) as f64 / col).clamp(0.0, cols as f64);
            let z = ((((start + self.width).min(to + 1)) - from) as f64 / col).clamp(0.0, cols as f64);
            // A bucket wider than a column (a short log) is where it
            // starts: spread, it would look busy all along.
            if z <= a || z - a > 2.0 {
                let c = &mut out[(a as usize).min(cols - 1)];
                for k in 0..3 {
                    c[k] += b[k] as f64;
                }
                continue;
            }
            let mut x = a;
            while x < z {
                let next = (x.floor() + 1.0).min(z);
                let share = (next - x) / (z - a);
                let c = &mut out[(x as usize).min(cols - 1)];
                for k in 0..3 {
                    c[k] += b[k] as f64 * share;
                }
                x = next;
            }
        }
        // Rounded up, so a column with a little in it still shows it.
        out.into_iter().map(|c| c.map(|n| if n > 0.0 { n.ceil() as u32 } else { 0 })).collect()
    }

    /// How wide a column is over `from..=to` in `cols` columns.
    pub fn column_width(from: i64, to: i64, cols: usize) -> i64 {
        ((to - from).max(1) / cols.max(1) as i64).max(1)
    }
}

/// A bar's character for `n` of `most`: none for nothing, then the eight
/// heights of a block.
pub fn bar(n: u32, most: u32) -> char {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if n == 0 || most == 0 {
        return ' ';
    }
    BARS[((n as u64 * 8).div_ceil(most as u64) as usize).clamp(1, 8) - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_widen_as_times_spread_and_keep_their_counts() {
        let mut h = Histogram::default();
        for s in 0..10_000i64 {
            h.add(1_000_000 + s * 1000, if s % 100 == 0 { Level::Error } else { Level::Info });
        }
        h.add(500_000, Level::Warn);
        let total: u32 = h.buckets.iter().map(|b| b[0]).sum();
        assert_eq!(total, 10_001);
        assert_eq!(h.buckets.iter().map(|b| b[2]).sum::<u32>(), 100);
        assert_eq!(h.buckets.len(), BUCKETS);
        assert_eq!(h.range, Some((500_000, 1_000_000 + 9_999_000)));
        let cols = h.columns(500_000, 10_999_000, 10);
        let sum = cols.iter().map(|c| c[0]).sum::<u32>();
        assert!((10_001..10_011).contains(&sum), "each column rounded up: {sum}");
        let even = h.columns(1_000_000, 10_999_000, 10);
        assert!(even.iter().all(|c| (999..=1002).contains(&c[0])), "spread evenly: {even:?}");
        assert!(cols[0][1] == 1, "the warning is in the first column: {cols:?}");
    }

    #[test]
    fn bars_rise_with_their_counts() {
        assert_eq!(bar(0, 10), ' ');
        assert_eq!(bar(1, 100), '▁');
        assert_eq!(bar(50, 100), '▄');
        assert_eq!(bar(100, 100), '█');
    }
}
