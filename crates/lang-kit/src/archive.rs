//! Unpacking a release in the sandbox: `.zip` (and `.vsix`), `.tar.gz`,
//! `.tar.xz`, `.tar`, and a lone `.gz`. A step at a time, so no call
//! into the extension runs past its deadline nor holds the archive in
//! memory: [`Unpack::step`] writes about as many bytes as it's given and
//! says whether there's more.
//!
//! A path that would land outside the folder (`..`, an absolute one) is
//! refused; links are left out (WASI makes none), and named in
//! `skipped`.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

/// What an archive is, by its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Zip,
    TarGz,
    TarXz,
    Tar,
    /// One file, gzipped: `rust-analyzer-x86_64-unknown-linux-gnu.gz`.
    Gz,
    /// Not an archive: the program itself, a `.jar`.
    Plain,
}

impl Kind {
    pub fn from_name(name: &str) -> Kind {
        let n = name.rsplit('/').next().unwrap_or(name).split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
        if n.ends_with(".zip") || n.ends_with(".vsix") || n.ends_with(".nupkg") {
            Kind::Zip
        } else if n.ends_with(".tar.gz") || n.ends_with(".tgz") {
            Kind::TarGz
        } else if n.ends_with(".tar.xz") || n.ends_with(".txz") {
            Kind::TarXz
        } else if n.ends_with(".tar") {
            Kind::Tar
        } else if n.ends_with(".gz") {
            Kind::Gz
        } else {
            Kind::Plain
        }
    }
}

/// The most an xz stream's dictionary may take: the extension has 128 MiB
/// in all.
const XZ_MEMORY_KB: u32 = 80 * 1024;

/// How many entries a step takes at most, however small they are.
const ENTRIES_PER_STEP: usize = 4000;

enum Stage {
    /// A compressed tar, written out as a plain one first.
    Inflate { from: Box<dyn Read>, to: File, tar: PathBuf },
    Tar { path: PathBuf, offset: u64, temporary: bool },
    Zip { zip: Box<zip::ZipArchive<File>>, next: usize },
    /// One file: decompressed or copied to `to`.
    Single { from: Box<dyn Read>, to: File, name: String },
    Done,
}

/// An archive being unpacked into a folder.
pub struct Unpack {
    dest: PathBuf,
    /// Leading folders taken off each path (`tar --strip-components`).
    strip: usize,
    stage: Stage,
    /// What's been written, relative to the folder, `/` between.
    pub files: Vec<String>,
    /// Those the archive marks as programs (a mode with `x`).
    pub executables: Vec<String>,
    /// Links and the like, left out.
    pub skipped: Vec<String>,
}

fn err(path: &Path, e: impl std::fmt::Display) -> String {
    format!("{}: {e}", path.display())
}

impl Unpack {
    /// `archive`, of `kind`, into `dest` (made if it isn't there). A lone
    /// file (`Gz`, `Plain`) is written as `dest/name`.
    pub fn new(archive: &Path, kind: Kind, dest: &Path, strip: usize, name: &str) -> Result<Unpack, String> {
        fs::create_dir_all(dest).map_err(|e| err(dest, e))?;
        let open = || File::open(archive).map_err(|e| err(archive, e));
        let single = |from: Box<dyn Read>| -> Result<Stage, String> {
            let name = clean(name, 0)?.ok_or_else(|| format!("{name}: not a name a file can have here"))?;
            let to_path = dest.join(&name);
            let to = File::create(&to_path).map_err(|e| err(&to_path, e))?;
            Ok(Stage::Single { from, to, name })
        };
        let inflate = |from: Box<dyn Read>| -> Result<Stage, String> {
            let tar = archive.with_extension("unpacking.tar");
            let to = File::create(&tar).map_err(|e| err(&tar, e))?;
            Ok(Stage::Inflate { from, to, tar })
        };
        let stage = match kind {
            Kind::Zip => {
                let zip = zip::ZipArchive::new(open()?).map_err(|e| err(archive, e))?;
                Stage::Zip { zip: Box::new(zip), next: 0 }
            }
            Kind::TarGz => inflate(Box::new(flate2::read::MultiGzDecoder::new(io::BufReader::new(open()?))))?,
            Kind::TarXz => inflate(Box::new(lzma_rust2::XzReader::new_mem_limit(io::BufReader::new(open()?), true, XZ_MEMORY_KB)))?,
            Kind::Tar => Stage::Tar { path: archive.to_path_buf(), offset: 0, temporary: false },
            Kind::Gz => single(Box::new(flate2::read::MultiGzDecoder::new(io::BufReader::new(open()?))))?,
            Kind::Plain => single(Box::new(open()?))?,
        };
        Ok(Unpack { dest: dest.to_path_buf(), strip, stage, files: Vec::new(), executables: Vec::new(), skipped: Vec::new() })
    }

    pub fn is_done(&self) -> bool {
        matches!(self.stage, Stage::Done)
    }

    /// Unpacks about `budget` bytes more; true once it's all there.
    pub fn step(&mut self, budget: u64) -> Result<bool, String> {
        let budget = budget.max(1);
        let stage = std::mem::replace(&mut self.stage, Stage::Done);
        self.stage = match stage {
            Stage::Done => Stage::Done,
            Stage::Inflate { mut from, mut to, tar } => {
                let n = io::copy(&mut from.as_mut().take(budget), &mut to).map_err(|e| err(&tar, e))?;
                if n < budget {
                    to.flush().map_err(|e| err(&tar, e))?;
                    Stage::Tar { path: tar, offset: 0, temporary: true }
                } else {
                    Stage::Inflate { from, to, tar }
                }
            }
            Stage::Single { mut from, mut to, name } => {
                let n = io::copy(&mut from.as_mut().take(budget), &mut to).map_err(|e| err(&self.dest.join(&name), e))?;
                if n < budget {
                    self.files.push(name);
                    Stage::Done
                } else {
                    Stage::Single { from, to, name }
                }
            }
            Stage::Tar { path, offset, temporary } => match self.tar_step(&path, offset, budget)? {
                Some(offset) => Stage::Tar { path, offset, temporary },
                None => {
                    if temporary {
                        let _ = fs::remove_file(&path);
                    }
                    Stage::Done
                }
            },
            Stage::Zip { mut zip, next } => {
                let next = self.zip_step(&mut zip, next, budget)?;
                if next < zip.len() {
                    Stage::Zip { zip, next }
                } else {
                    Stage::Done
                }
            }
        };
        Ok(self.is_done())
    }

    /// Everything, in one go: for tests, and small archives.
    pub fn run(&mut self) -> Result<(), String> {
        while !self.step(64 << 20)? {}
        Ok(())
    }

    /// Entries from `offset` on, until `budget` is spent: where to go on
    /// from, or `None` at the end.
    fn tar_step(&mut self, path: &Path, offset: u64, budget: u64) -> Result<Option<u64>, String> {
        let mut file = File::open(path).map_err(|e| err(path, e))?;
        file.seek(SeekFrom::Start(offset)).map_err(|e| err(path, e))?;
        let mut archive = tar::Archive::new(file);
        let entries = archive.entries().map_err(|e| err(path, e))?;
        let (mut spent, mut taken) = (0u64, 0usize);
        for entry in entries {
            let mut entry = entry.map_err(|e| err(path, e))?;
            let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
            let size = entry.size();
            let end = offset + entry.raw_file_position() + size.div_ceil(512) * 512;
            let kind = entry.header().entry_type();
            match clean(&name, self.strip)? {
                None => {}
                Some(rel) => {
                    let to = self.dest.join(&rel);
                    if kind.is_dir() {
                        fs::create_dir_all(&to).map_err(|e| err(&to, e))?;
                    } else if kind.is_file() || kind == tar::EntryType::Continuous {
                        if let Some(parent) = to.parent() {
                            fs::create_dir_all(parent).map_err(|e| err(parent, e))?;
                        }
                        let mut out = File::create(&to).map_err(|e| err(&to, e))?;
                        io::copy(&mut entry, &mut out).map_err(|e| err(&to, e))?;
                        if entry.header().mode().is_ok_and(|m| m & 0o111 != 0) {
                            self.executables.push(rel.clone());
                        }
                        self.files.push(rel);
                    } else if !matches!(kind, tar::EntryType::XGlobalHeader | tar::EntryType::XHeader | tar::EntryType::GNULongName | tar::EntryType::GNULongLink) {
                        self.skipped.push(rel);
                    }
                }
            }
            spent += size;
            taken += 1;
            if spent >= budget || taken >= ENTRIES_PER_STEP {
                return Ok(Some(end));
            }
        }
        Ok(None)
    }

    /// Entries from `next` on, until `budget` is spent: the next one.
    fn zip_step(&mut self, zip: &mut zip::ZipArchive<File>, mut next: usize, budget: u64) -> Result<usize, String> {
        let (mut spent, mut taken) = (0u64, 0usize);
        while next < zip.len() && spent < budget && taken < ENTRIES_PER_STEP {
            let mut entry = zip.by_index(next).map_err(|e| format!("entry {next}: {e}"))?;
            next += 1;
            taken += 1;
            let name = entry.name().map_err(|e| format!("entry {next}: {e}"))?.to_string();
            if entry.enclosed_name().is_none() {
                return Err(format!("{name}: outside the folder it's unpacked into"));
            }
            let Some(rel) = clean(&name, self.strip)? else { continue };
            let to = self.dest.join(&rel);
            if entry.is_dir() {
                fs::create_dir_all(&to).map_err(|e| err(&to, e))?;
                continue;
            }
            if entry.is_symlink() {
                self.skipped.push(rel);
                continue;
            }
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).map_err(|e| err(parent, e))?;
            }
            let mut out = File::create(&to).map_err(|e| err(&to, e))?;
            spent += io::copy(&mut entry, &mut out).map_err(|e| err(&to, e))?;
            if entry.unix_mode().is_some_and(|m| m & 0o111 != 0) {
                self.executables.push(rel.clone());
            }
            self.files.push(rel);
        }
        Ok(next)
    }
}

/// Where `path` goes, with its first `strip` folders taken off, as a
/// relative path with `/` between: `Err` when it would leave the folder,
/// `Ok(None)` when nothing's left of it (a top folder stripped).
pub fn clean(path: &str, strip: usize) -> Result<Option<String>, String> {
    let mut parts = Vec::new();
    for c in Path::new(&path.replace('\\', "/")).components() {
        match c {
            Component::Normal(p) => parts.push(p.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return Err(format!("{path}: outside the folder it's unpacked into")),
        }
    }
    Ok((parts.len() > strip).then(|| parts[strip..].join("/")))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lang-kit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The files of a release: a program, its library, a readme.
    pub const FILES: [(&str, &[u8], u32); 3] = [("server-1.0/bin/server", b"#!/bin/sh\necho hi\n", 0o755), ("server-1.0/lib/core.js", b"module.exports = 1\n", 0o644), ("server-1.0/README.md", b"# Server\n", 0o644)];

    pub fn tar_bytes() -> Vec<u8> {
        let mut b = tar::Builder::new(Vec::new());
        let mut dir = tar::Header::new_gnu();
        dir.set_entry_type(tar::EntryType::Directory);
        dir.set_mode(0o755);
        dir.set_size(0);
        b.append_data(&mut dir, "server-1.0/", io::empty()).unwrap();
        for (name, data, mode) in FILES {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(mode);
            b.append_data(&mut h, name, data).unwrap();
        }
        // A link, and a long name (a GNU extension entry before it).
        let mut l = tar::Header::new_gnu();
        l.set_entry_type(tar::EntryType::Symlink);
        l.set_size(0);
        b.append_link(&mut l, "server-1.0/bin/srv", "server").unwrap();
        let long = format!("server-1.0/{}/deep.txt", "d".repeat(120));
        let mut h = tar::Header::new_gnu();
        h.set_size(4);
        h.set_mode(0o644);
        b.append_data(&mut h, &long, &b"deep"[..]).unwrap();
        b.into_inner().unwrap()
    }

    pub fn tar_gz_bytes() -> Vec<u8> {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&tar_bytes()).unwrap();
        gz.finish().unwrap()
    }

    pub fn tar_xz_bytes() -> Vec<u8> {
        let mut xz = lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::with_preset(6)).unwrap();
        xz.write_all(&tar_bytes()).unwrap();
        xz.finish().unwrap()
    }

    pub fn zip_bytes() -> Vec<u8> {
        let mut z = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
        z.add_directory("server-1.0/", zip::write::SimpleFileOptions::default()).unwrap();
        for (name, data, mode) in FILES {
            z.start_file(name, zip::write::SimpleFileOptions::default().unix_permissions(mode)).unwrap();
            z.write_all(data).unwrap();
        }
        z.add_symlink("server-1.0/bin/srv", "server", zip::write::SimpleFileOptions::default()).unwrap();
        z.finish().unwrap().into_inner()
    }

    fn unpacked(name: &str, bytes: &[u8], kind: Kind, strip: usize, budget: u64) -> (Unpack, PathBuf, usize) {
        let dir = temp(name);
        let archive = dir.join(format!("release.{name}"));
        fs::write(&archive, bytes).unwrap();
        let dest = dir.join("out");
        let mut u = Unpack::new(&archive, kind, &dest, strip, "server").unwrap();
        let mut steps = 1;
        while !u.step(budget).unwrap() {
            steps += 1;
            assert!(steps < 1000);
        }
        (u, dest, steps)
    }

    fn check(u: &Unpack, dest: &Path, prefix: &str) {
        for (name, data, _) in FILES {
            let rel = name.strip_prefix("server-1.0/").map(|r| format!("{prefix}{r}")).unwrap();
            assert_eq!(fs::read(dest.join(&rel)).unwrap(), data, "{rel}");
            assert!(u.files.contains(&rel), "{rel} in {:?}", u.files);
        }
        assert_eq!(u.executables, [format!("{prefix}bin/server")]);
        assert_eq!(u.skipped, [format!("{prefix}bin/srv")]);
        assert!(!dest.join(format!("{prefix}bin/srv")).exists());
    }

    #[test]
    fn kinds_come_from_names() {
        assert_eq!(Kind::from_name("https://x/a/clangd-linux-18.1.3.zip"), Kind::Zip);
        assert_eq!(Kind::from_name("codelldb-linux-x64.vsix"), Kind::Zip);
        assert_eq!(Kind::from_name("jdt-language-server-1.36.tar.gz"), Kind::TarGz);
        assert_eq!(Kind::from_name("x.TGZ?raw=1"), Kind::TarGz);
        assert_eq!(Kind::from_name("node-v20-linux-x64.tar.xz"), Kind::TarXz);
        assert_eq!(Kind::from_name("a.tar"), Kind::Tar);
        assert_eq!(Kind::from_name("rust-analyzer-x86_64-unknown-linux-gnu.gz"), Kind::Gz);
        assert_eq!(Kind::from_name("google-java-format-1.22.0-all-deps.jar"), Kind::Plain);
    }

    #[test]
    fn paths_stay_in_the_folder() {
        assert_eq!(clean("a/b/c", 0), Ok(Some("a/b/c".into())));
        assert_eq!(clean("./a/b/c", 1), Ok(Some("b/c".into())));
        assert_eq!(clean("a\\b", 0), Ok(Some("a/b".into())));
        assert_eq!(clean("a/", 1), Ok(None));
        assert!(clean("../etc/passwd", 0).is_err());
        assert!(clean("a/../../x", 0).is_err());
        assert!(clean("/etc/passwd", 0).is_err());
    }

    #[test]
    fn a_tar_gz_unpacks_a_step_at_a_time() {
        let (u, dest, steps) = unpacked("tgz", &tar_gz_bytes(), Kind::TarGz, 0, 16);
        check(&u, &dest, "server-1.0/");
        assert!(steps > 5, "{steps} steps");
        assert_eq!(fs::read(dest.join(format!("server-1.0/{}/deep.txt", "d".repeat(120)))).unwrap(), b"deep");
        assert!(!dest.parent().unwrap().join("release.unpacking.tar").exists(), "the plain tar is gone after");
    }

    #[test]
    fn a_tar_xz_unpacks_its_folder_stripped() {
        let (u, dest, steps) = unpacked("txz", &tar_xz_bytes(), Kind::TarXz, 1, 1 << 20);
        check(&u, &dest, "");
        assert_eq!(steps, 2, "inflated, then read");
    }

    #[test]
    fn a_plain_tar_unpacks_in_one_step() {
        let (u, dest, steps) = unpacked("tar", &tar_bytes(), Kind::Tar, 1, 1 << 20);
        check(&u, &dest, "");
        assert_eq!(steps, 1);
    }

    #[test]
    fn a_zip_unpacks_a_step_at_a_time() {
        let (u, dest, steps) = unpacked("zip", &zip_bytes(), Kind::Zip, 0, 1);
        check(&u, &dest, "server-1.0/");
        assert!(steps >= 3, "{steps}");
        let (u, dest, _) = unpacked("zip1", &zip_bytes(), Kind::Zip, 1, 1 << 20);
        check(&u, &dest, "");
    }

    #[test]
    fn a_lone_gz_or_a_plain_file_is_written_as_its_name() {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(b"\x7fELF...").unwrap();
        let (u, dest, _) = unpacked("gz", &gz.finish().unwrap(), Kind::Gz, 0, 3);
        assert_eq!(fs::read(dest.join("server")).unwrap(), b"\x7fELF...");
        assert_eq!(u.files, ["server"]);
        let (u, dest, _) = unpacked("plain", b"PK jar", Kind::Plain, 0, 1 << 20);
        assert_eq!((fs::read(dest.join("server")).unwrap(), u.files), (b"PK jar".to_vec(), vec!["server".to_string()]));
    }

    #[test]
    fn an_entry_out_of_the_folder_is_refused() {
        let mut b = tar::Builder::new(Vec::new());
        let mut h = tar::Header::new_gnu();
        h.set_size(1);
        h.set_mode(0o644);
        // The builder refuses `..` itself, so it's written into the header.
        h.as_gnu_mut().unwrap().name[..12].copy_from_slice(b"../escape.sh");
        h.set_cksum();
        b.append(&h, &b"x"[..]).unwrap();
        let dir = temp("evil");
        let archive = dir.join("evil.tar");
        fs::write(&archive, b.into_inner().unwrap()).unwrap();
        let mut u = Unpack::new(&archive, Kind::Tar, &dir.join("out"), 0, "x").unwrap();
        assert!(u.run().unwrap_err().contains("outside the folder"));
        assert!(!dir.join("escape.sh").exists());
        let bad = Unpack::new(&dir.join("missing.zip"), Kind::Zip, &dir.join("out"), 0, "x");
        assert!(bad.is_err());
        let corrupt = dir.join("corrupt.tar.gz");
        fs::write(&corrupt, b"not gzip at all").unwrap();
        let mut u = Unpack::new(&corrupt, Kind::TarGz, &dir.join("out2"), 0, "x").unwrap();
        assert!(u.run().is_err());
    }
}
