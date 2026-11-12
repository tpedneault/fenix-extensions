//! What the page says about the text: how much of it there is, and how
//! long it takes to read. No Fenix in it, so `cargo test` covers it all.

/// The reading speed, in words a minute, when the setting says nothing
/// usable.
pub const DEFAULT_SPEED: u32 = 200;
/// The fastest and slowest speed the setting is taken at.
pub const SPEEDS: std::ops::RangeInclusive<u32> = 20..=1000;

/// A text, counted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub characters: usize,
    pub words: usize,
    pub lines: usize,
}

impl Stats {
    /// Counts `text`: its characters (not bytes), its words (what
    /// whitespace separates) and its lines (none for no text).
    pub fn of(text: &str) -> Stats {
        Stats { characters: text.chars().count(), words: text.split_whitespace().count(), lines: if text.is_empty() { 0 } else { text.lines().count().max(1) + usize::from(text.ends_with('\n')) } }
    }

    /// Minutes to read it at `speed` words a minute, rounded up: a
    /// single word takes a minute, none take none.
    pub fn minutes(&self, speed: u32) -> usize {
        self.words.div_ceil(speed.max(1) as usize)
    }

    /// The counts in a line, as the page shows them.
    pub fn summary(&self, speed: u32) -> String {
        let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        format!("{} · {} · {} · about {} min to read", plural(self.words, "word", "words"), plural(self.characters, "character", "characters"), plural(self.lines, "line", "lines"), self.minutes(speed))
    }
}

/// The words a minute the setting holds: its text read as a number, held
/// to [`SPEEDS`], or [`DEFAULT_SPEED`] when it isn't one.
pub fn speed(setting: Option<&str>) -> u32 {
    setting.and_then(|s| s.trim().parse::<u32>().ok()).map(|n| n.clamp(*SPEEDS.start(), *SPEEDS.end())).unwrap_or(DEFAULT_SPEED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_counted_in_characters_words_and_lines() {
        assert_eq!(Stats::of(""), Stats::default());
        assert_eq!(Stats::of("one"), Stats { characters: 3, words: 1, lines: 1 });
        assert_eq!(Stats::of("two  words\nand more\n"), Stats { characters: 20, words: 4, lines: 3 });
        assert_eq!(Stats::of("héllo wörld").characters, 11, "characters, not bytes");
    }

    #[test]
    fn reading_time_rounds_up() {
        let words = |n: usize| Stats { words: n, ..Stats::default() };
        assert_eq!([0, 1, 200, 201, 400].map(|n| words(n).minutes(200)), [0, 1, 1, 2, 2]);
        assert_eq!(words(100).minutes(0), 100, "a speed of nothing is taken as one a minute");
    }

    #[test]
    fn the_summary_says_one_in_the_singular() {
        assert_eq!(Stats::of("one").summary(200), "1 word · 3 characters · 1 line · about 1 min to read");
        assert_eq!(Stats::of("").summary(200), "0 words · 0 characters · 0 lines · about 0 min to read");
    }

    #[test]
    fn the_setting_is_a_number_held_to_a_range() {
        assert_eq!([speed(None), speed(Some("250")), speed(Some(" 300 ")), speed(Some("fast")), speed(Some("1")), speed(Some("99999"))], [200, 250, 300, 200, 20, 1000]);
    }
}
