//! A million rows, timed: how long the table takes to read, draw, sort,
//! filter, sum up and change.
//!
//! ```text
//! cargo run --release --example million
//! ```

use std::time::Instant;

use fenix_csv::dialect::{detect, Dialect};
use fenix_csv::edit::text_edits;
use fenix_csv::page::TablePage;
use fenix_csv::table::Table;
use fenix_csv::view::{self, parse_filter, Sort};
use fenix_ui_kit::Key;

const CITIES: [&str; 6] = ["London", "Paris", "Montréal", "New York", "Tokyo", "São Paulo"];

fn time<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let out = f();
    println!("{what:<40} {:>8.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    out
}

fn main() {
    let rows = 1_000_000;
    let mut text = String::from("id,name,price,city,note\n");
    let mut seed = 12345u64;
    for i in 0..rows {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let price = (seed >> 33) % 100_000;
        let city = CITIES[(seed >> 20) as usize % CITIES.len()];
        let note = if i % 10 == 0 { "\"has, a comma\"" } else { "plain" };
        text.push_str(&format!("{i},name {},{}.{:02},{city},{note}\n", seed % 9973, price / 100, price % 100));
    }
    println!("{} rows, {:.1} MiB", rows, text.len() as f64 / (1024.0 * 1024.0));

    let d = time("guess the dialect", || detect(&text, false));
    assert_eq!(d, Dialect::default());
    let table = time("find the records", || Table::new(text.clone(), d));
    assert_eq!(table.rows(), rows);

    let mut page = time("open the page", || TablePage::new("/m.csv", "/m.csv", text.clone().into_bytes(), None));
    time("draw the first screen", || page.render(160));
    struct No;
    impl fenix_csv::page::World for No {
        fn buffer_text(&mut self, _: &str) -> Option<String> {
            None
        }
        fn edit(&mut self, _: &str, _: Vec<fenix_csv::edit::TextEdit>) -> Result<(), String> {
            Ok(())
        }
        fn save(&mut self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn copy(&mut self, _: &str) {}
        fn say(&mut self, _: &str) {}
        fn text(&mut self, _: &str) {}
        fn remember(&mut self, _: &str, _: Option<Dialect>) {}
        fn buffer_undo(&mut self, _: &str, _: bool) -> Result<(), String> {
            Ok(())
        }
    }
    time("G, and draw the last screen", || {
        page.key(Key::Char('G'), &mut No);
        page.render(160)
    });

    page.rows_shown = Some(8);
    page.key(Key::Char('c'), &mut No);
    page.key(Key::Char('l'), &mut No);
    page.key(Key::Char('l'), &mut No);
    println!("\n{}\n", page.render(100).text());
    time("read every row's name", || (0..table.rows()).map(|r| table.cell(r, 1)).filter(|c| !c.is_empty()).count());
    time("read every row's note (some quoted)", || (0..table.rows()).map(|r| table.cell(r, 4)).filter(|c| !c.is_empty()).count());
    time("sort by price (numbers)", || view::rows(&table, None, Some(Sort { col: 2, desc: false })));
    time("sort by name (text)", || view::rows(&table, None, Some(Sort { col: 1, desc: true })));
    let titles = table.titles();
    time("filter: a text", || view::rows(&table, parse_filter("tokyo", &titles).as_ref(), None));
    time("filter: price > 500 & city ~ o", || view::rows(&table, parse_filter("price > 500 & city ~ o", &titles).as_ref(), None));
    time("stats on price", || view::stats(&table, 0..table.rows(), 2));
    time("stats on city", || view::stats(&table, 0..table.rows(), 3));

    let mut t = table.clone();
    let s = t.set_cell(rows - 1, 1, "the last, changed").unwrap();
    time("the last row's cell as the buffer's edit", || text_edits(t.text(), &s));
    time("  and made to the table", || t.apply(&s));
    time("a row added at the top", || {
        let s = t.insert_row(0);
        t.apply(&s)
    });
    time("e on a cell, typed, Enter (through a page)", || {
        page.key(Key::Char('g'), &mut No);
        page.key(Key::Char('e'), &mut No);
        page.paste("x");
        page.key(Key::Enter, &mut No);
        page.render(160)
    });
}
