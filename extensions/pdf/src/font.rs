//! The letters link hints are labelled with (`f`), drawn into the page's
//! pixels: a surface has no text of its own, and the labels only ever use
//! the home row's letters (`text::HINT_KEYS`), so a tiny bitmap font is
//! all it takes.

/// Each letter, 5 x 7, a row a string: `#` is ink.
const GLYPHS: &[(char, [&str; 7])] = &[
    ('a', [".....", ".....", ".###.", "....#", ".####", "#...#", ".####"]),
    ('s', [".....", ".....", ".####", "#....", ".###.", "....#", "####."]),
    ('d', ["....#", "....#", ".####", "#...#", "#...#", "#...#", ".####"]),
    ('f', ["..##.", ".#...", "####.", ".#...", ".#...", ".#...", ".#..."]),
    ('g', [".....", ".####", "#...#", "#...#", ".####", "....#", ".###."]),
    ('h', ["#....", "#....", "####.", "#...#", "#...#", "#...#", "#...#"]),
    ('j', ["...#.", ".....", "..##.", "...#.", "...#.", "#..#.", ".##.."]),
    ('k', ["#....", "#....", "#..#.", "#.#..", "##...", "#.#..", "#..#."]),
    ('l', [".##..", "..#..", "..#..", "..#..", "..#..", "..#..", ".###."]),
];

/// The label's ink: Cinder, the Fenix mark's red, as 1.x drew hints.
pub const INK: (u8, u8, u8) = (0xd6, 0x3a, 0x2f);
/// Behind it, so it reads on any page.
pub const BACK: (u8, u8, u8) = (0xff, 0xec, 0xb3);

/// `text` drawn as a label `scale` pixels a dot: its size, and BGRA.
/// Letters it has no glyph for are left blank.
pub fn label(text: &str, scale: u32) -> (u32, u32, Vec<u8>) {
    let s = scale.max(1);
    let n = text.chars().count() as u32;
    // A dot of padding all round, a dot between letters.
    let (w, h) = ((n * 6 + 1) * s, 9 * s);
    let mut bgra = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..w * h {
        bgra.extend_from_slice(&[BACK.2, BACK.1, BACK.0, 255]);
    }
    for (i, c) in text.chars().enumerate() {
        let Some((_, rows)) = GLYPHS.iter().find(|(g, _)| *g == c) else { continue };
        for (row, line) in rows.iter().enumerate() {
            for (col, dot) in line.chars().enumerate() {
                if dot != '#' {
                    continue;
                }
                let (x0, y0) = ((1 + i as u32 * 6 + col as u32) * s, (1 + row as u32) * s);
                for y in y0..y0 + s {
                    for x in x0..x0 + s {
                        let at = ((y * w + x) * 4) as usize;
                        bgra[at..at + 4].copy_from_slice(&[INK.2, INK.1, INK.0, 255]);
                    }
                }
            }
        }
    }
    (w, h, bgra)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hint_letter_has_a_glyph() {
        for c in crate::text::HINT_KEYS {
            assert!(GLYPHS.iter().any(|(g, rows)| g == c && rows.iter().all(|r| r.len() == 5)), "{c}");
        }
    }

    #[test]
    fn a_label_is_its_letters_in_ink_on_a_box() {
        let (w, h, bgra) = label("ad", 2);
        assert_eq!((w, h), (26, 18));
        assert_eq!(bgra.len(), (w * h * 4) as usize);
        let px = |x: u32, y: u32| &bgra[((y * w + x) * 4) as usize..((y * w + x) * 4 + 4) as usize];
        assert_eq!(px(0, 0), &[BACK.2, BACK.1, BACK.0, 255], "the box");
        // `a`'s third row, second column: ink.
        assert_eq!(px(2 + 2, 2 + 4), &[INK.2, INK.1, INK.0, 255]);
        // `d`'s top right: ink, a letter along.
        assert_eq!(px((1 + 6 + 4) * 2, 2), &[INK.2, INK.1, INK.0, 255]);
    }
}
