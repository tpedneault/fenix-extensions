//! SyncTeX's forward search: which page of the PDF the line you're on is
//! on, asked of the `synctex` program (it comes with a TeX distribution; the
//! build writes `main.synctex.gz` beside the PDF).
//!
//! `synctex view -i LINE:COLUMN:FILE -o main.pdf` prints, between `SyncTeX
//! result begin` and `end`, the page and the place on it (`Page:3`, `x:`,
//! `y:`, `h:`, `v:`, `W:`, `H:`). The PDF reader's `pdf.open` takes a page and
//! no place, so the page is what's used; the place is read and kept for
//! a reader that takes one.

use fenix_extension_api::Command;

/// What `synctex view` found.
#[derive(Debug, Clone, PartialEq)]
pub struct Spot {
    pub page: u32,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

/// `synctex view` for line `line` of `file` (in the project) in `pdf`. The
/// column is 0: the line is all the cursor's place the pack has.
pub fn view(synctex: &str, line: u32, file: &str, pdf: &str) -> Command {
    Command::new(synctex).args(["view", "-i", &format!("{line}:0:{file}"), "-o", pdf])
}

/// The first result in what it printed.
pub fn parse(output: &str) -> Option<Spot> {
    let mut inside = false;
    let (mut page, mut x, mut y) = (None, None, None);
    for line in output.lines() {
        let line = line.trim();
        if line == "SyncTeX result begin" {
            inside = true;
        } else if line == "SyncTeX result end" {
            break;
        } else if inside {
            if let Some(v) = line.strip_prefix("Page:") {
                page = page.or(v.trim().parse().ok());
            } else if let Some(v) = line.strip_prefix("x:") {
                x = x.or(v.trim().parse().ok());
            } else if let Some(v) = line.strip_prefix("y:") {
                y = y.or(v.trim().parse().ok());
            }
        }
    }
    page.map(|page| Spot { page, x, y })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Written from SyncTeX's documented output (synctex 1.5), not recorded:
    // there's no `synctex` here.
    const FOUND: &str = "This is SyncTeX command line utility, version 1.5\nSyncTeX result begin\nOutput:main.pdf\nPage:3\nx:133.768356\ny:134.764618\nh:133.768356\nv:134.764618\nW:343.711060\nH:9.962640\nbefore:\noffset:0\nmiddle:\nafter:\nSyncTeX result end\n";

    #[test]
    fn the_page_and_the_place_are_read() {
        assert_eq!(parse(FOUND), Some(Spot { page: 3, x: Some(133.768356), y: Some(134.764618) }));
        let two = FOUND.replace("SyncTeX result end\n", "SyncTeX result end\nSyncTeX result begin\nPage:9\nSyncTeX result end\n");
        assert_eq!(parse(&two).unwrap().page, 3, "the first");
    }

    #[test]
    fn no_result_is_none() {
        assert_eq!(parse("This is SyncTeX command line utility, version 1.5\nSyncTeX result begin\nSyncTeX result end\n"), None);
        assert_eq!(parse("SyncTeX Warning: No tag for input file main.tex\n"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn the_command_names_the_line_the_file_and_the_pdf() {
        assert_eq!(fenix_test_kit::line(&view("/t/synctex", 12, "chapters/a.tex", "main.pdf")), "/t/synctex view -i 12:0:chapters/a.tex -o main.pdf");
    }
}
