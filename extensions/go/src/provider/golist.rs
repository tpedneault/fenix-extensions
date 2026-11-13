//! `go list -e -f '{{.ImportPath}}<tab>{{.Dir}}<tab>{{join .TestGoFiles ","}}<tab>{{join .XTestGoFiles ","}}'`:
//! each package and the test files Go would build for it, without
//! building anything, even when the code doesn't compile.
//!
//! It's the listing: `go test -list` builds every test binary and gives
//! no lines, so the listing is of files instead. A file the scan has seen
//! (Fenix scans each of a project's first 2,000 as the provider starts)
//! is skipped; one it hasn't (past that, or in a folder Fenix's walk left
//! out) is read from the project's folder and scanned here, its tests
//! found as a scan finds them. A few at a time, a call's few milliseconds
//! each; what's left is read when the step ends, up to [`FILES_MAX`] in
//! all (more are left to Fenix's own scans, when the files are opened or
//! run). Go's own file list also honours build constraints (`//go:build
//! integration`), which the scanner only reads at the top of the file: a
//! file it names is one that builds.

use std::collections::VecDeque;

use fenix_lang_kit::tests::{Line, Reader, Report, StepEnd};

use super::read::Context;
use super::scan_file;

/// Files read in one call of the reader.
const PER_CALL: usize = 40;

/// Files read in one step, most.
pub const FILES_MAX: usize = 2000;

pub struct ListReader {
    ctx: Context,
    /// Files to read, below the project.
    pending: VecDeque<String>,
    read: usize,
    /// Lines that weren't a package's: go's own errors.
    noise: Vec<String>,
}

impl ListReader {
    pub fn new(ctx: Context) -> ListReader {
        ListReader { ctx, pending: VecDeque::new(), read: 0, noise: Vec::new() }
    }

    /// Reads and scans up to `n` of the files waiting.
    fn work(&mut self, n: usize, out: &mut Vec<Report>) {
        for _ in 0..n {
            let Some(rel) = self.pending.pop_front() else { return };
            if self.read >= FILES_MAX || self.ctx.index.borrow().is_scanned(&rel) {
                continue;
            }
            self.read += 1;
            let Ok(text) = std::fs::read_to_string(format!("{}/{rel}", self.ctx.index.borrow().modules.root)) else { continue };
            out.extend(scan_file(&self.ctx.index, &rel, &text).into_iter().map(Report::Found));
        }
    }
}

impl Reader for ListReader {
    fn line(&mut self, line: Line, out: &mut Vec<Report>) {
        let fields: Vec<&str> = line.text.split('\t').collect();
        match fields[..] {
            [package, _dir, files, xfiles] if !line.stderr => {
                let dir = {
                    let mut index = self.ctx.index.borrow_mut();
                    index.package(package).map(|p| p.dir.clone()).or_else(|| index.modules.package_dir(package))
                };
                if let Some(dir) = dir {
                    for file in files.split(',').chain(xfiles.split(',')).filter(|f| f.ends_with("_test.go")) {
                        self.pending.push_back(if dir.is_empty() { file.to_string() } else { format!("{dir}/{file}") });
                    }
                }
            }
            _ if self.noise.len() < 50 && !line.text.trim().is_empty() => self.noise.push(line.text.to_string()),
            _ => {}
        }
        self.work(PER_CALL, out);
    }

    fn end(&mut self, end: &StepEnd, out: &mut Vec<Report>) {
        let all = self.pending.len();
        self.work(all, out);
        if matches!(end, StepEnd::Exited(Some(c)) if *c != 0) {
            if let Some(first) = self.noise.iter().find(|l| !l.starts_with("go: downloading ")) {
                out.push(Report::Broken { under: None, message: first.trim().to_string(), at: None });
            }
        }
    }
}
