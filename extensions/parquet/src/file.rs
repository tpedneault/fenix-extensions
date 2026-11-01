//! A Parquet file as the page reads it: its footer once, when it opens,
//! then rows only as they're shown -- the row groups that hold them,
//! and, with the file's page index, only the pages that do -- so a
//! billion-row file opens and jumps to its end as a thousand-row one
//! does.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Seek, SeekFrom};

use arrow_array::{Array, ArrayRef, UInt64Array};
use arrow_cast::display::{ArrayFormatter, FormatOptions};
use arrow_schema::{DataType, SortOptions};
use bytes::{Buf, Bytes};
use parquet::arrow::arrow_reader::statistics::StatisticsConverter;
use parquet::arrow::arrow_reader::{ArrowReaderMetadata, ArrowReaderOptions, ParquetRecordBatchReaderBuilder};
use parquet::errors::ParquetError;
use parquet::file::reader::{ChunkReader, Length};

/// Rows are read this many at a time.
const CHUNK: usize = 200;
/// Chunks kept read, the least lately shown going first.
const KEPT: usize = 16;
/// A cell's text is cut to this many characters.
const CELL_MAX: usize = 4096;

/// Where a file's bytes are read from: a file in the project, as far
/// as each read needs, or the whole file, which Fenix read for it.
#[derive(Clone)]
pub enum Input {
    /// A path under `/project`, and the file's length.
    Path(String, u64),
    Bytes(Bytes),
}

impl Length for Input {
    fn len(&self) -> u64 {
        match self {
            Input::Path(_, len) => *len,
            Input::Bytes(b) => b.len() as u64,
        }
    }
}

impl ChunkReader for Input {
    type T = Box<dyn Read + Send>;

    fn get_read(&self, start: u64) -> parquet::errors::Result<Self::T> {
        match self {
            Input::Path(path, _) => {
                let mut f = std::fs::File::open(path)?;
                f.seek(SeekFrom::Start(start))?;
                Ok(Box::new(std::io::BufReader::new(f)))
            }
            Input::Bytes(b) => {
                let start = (start as usize).min(b.len());
                Ok(Box::new(b.slice(start..).reader()))
            }
        }
    }

    fn get_bytes(&self, start: u64, length: usize) -> parquet::errors::Result<Bytes> {
        match self {
            Input::Path(path, _) => {
                let mut f = std::fs::File::open(path)?;
                f.seek(SeekFrom::Start(start))?;
                let mut buf = vec![0; length];
                f.read_exact(&mut buf)?;
                Ok(buf.into())
            }
            Input::Bytes(b) => {
                let start = start as usize;
                if start.checked_add(length).is_none_or(|end| end > b.len()) {
                    return Err(ParquetError::EOF(format!("{length} bytes at {start}, past the file's {}", b.len())));
                }
                Ok(b.slice(start..start + length))
            }
        }
    }
}

/// One of the file's columns -- a top-level field, a nested one's
/// leaves taken together -- as its schema says and its footer counts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColumnInfo {
    pub name: String,
    /// Its Arrow type: `Int64`, `Utf8`, `Timestamp(µs, "UTC")`, `List(Utf8)`.
    pub kind: String,
    /// Its Parquet type, as it's stored: `INT64`, `BYTE_ARRAY (String)`.
    pub stored: String,
    pub nullable: bool,
    /// A number: lined up on the right.
    pub numeric: bool,
    /// Its nulls, when every row group's statistics count them.
    pub nulls: Option<u64>,
    /// Its smallest and largest values, as the statistics have them.
    pub min: Option<String>,
    pub max: Option<String>,
    /// Bytes on disk, and decompressed.
    pub compressed: u64,
    pub uncompressed: u64,
    /// The codecs its chunks are compressed with.
    pub codecs: Vec<String>,
}

/// The file's footer, as the header says it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileInfo {
    pub rows: u64,
    pub row_groups: usize,
    pub size: u64,
    pub version: i32,
    pub created_by: Option<String>,
    /// Its key-value metadata's keys: `pandas`, `ARROW:schema`.
    pub keys: Vec<String>,
    /// The codecs any column uses.
    pub codecs: Vec<String>,
}

/// A row: each column's text, `None` for a null.
pub type Row = Vec<Option<String>>;

#[derive(Default)]
struct Cache {
    chunks: HashMap<usize, Vec<Row>>,
    order: VecDeque<usize>,
    /// The last read that failed, and why.
    error: Option<String>,
}

pub struct ParquetFile {
    input: Input,
    meta: ArrowReaderMetadata,
    /// The row each row group starts at, and the rows after the last.
    starts: Vec<u64>,
    pub info: FileInfo,
    pub columns: Vec<ColumnInfo>,
    cache: RefCell<Cache>,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// `n` bytes, as people say them: `512 B`, `34.9 KB`, `1.2 GB`.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit < UNITS.len() - 1 {
        v /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

/// `n` with its thousands apart: `1,000,000`.
pub fn count(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn is_numeric(t: &DataType) -> bool {
    t.is_numeric() || matches!(t, DataType::Decimal32(..) | DataType::Decimal64(..) | DataType::Decimal128(..) | DataType::Decimal256(..))
}

/// Cell `i` of `array`, as its type writes it: dates and times in ISO
/// form, decimals at their scale, lists and structs as `[..]` and `{..}`.
fn cell(f: &ArrayFormatter, array: &dyn Array, i: usize) -> Option<String> {
    if array.is_null(i) {
        return None;
    }
    let mut s = f.value(i).to_string();
    if s.chars().count() > CELL_MAX {
        s = s.chars().take(CELL_MAX).collect::<String>() + "…";
    }
    Some(s)
}

/// The value of `values` that sorts first (`descending`: last), nulls
/// aside, as its type writes it.
fn extreme(values: &ArrayRef, descending: bool) -> Option<String> {
    let at = arrow_ord::sort::sort_to_indices(values, Some(SortOptions { descending, nulls_first: false }), Some(1)).ok()?;
    let i = at.values().first().copied()? as usize;
    let f = ArrayFormatter::try_new(values.as_ref(), &FormatOptions::default()).ok()?;
    cell(&f, values.as_ref(), i)
}

impl ParquetFile {
    pub fn open(input: Input) -> Result<ParquetFile, String> {
        let options = ArrowReaderOptions::new().with_page_index(true);
        let meta = ArrowReaderMetadata::load(&input, options).map_err(err)?;
        let md = meta.metadata().clone();
        let fm = md.file_metadata();
        let schema = meta.schema().clone();
        let parquet_schema = fm.schema_descr();

        let mut starts = Vec::with_capacity(md.num_row_groups() + 1);
        let mut at = 0u64;
        for rg in md.row_groups() {
            starts.push(at);
            at += rg.num_rows().max(0) as u64;
        }
        starts.push(at);

        let mut columns: Vec<ColumnInfo> = schema
            .fields()
            .iter()
            .map(|f| ColumnInfo { name: f.name().clone(), kind: f.data_type().to_string(), nullable: f.is_nullable(), numeric: is_numeric(f.data_type()), ..ColumnInfo::default() })
            .collect();
        // What each leaf adds to its top-level column: bytes and codecs.
        for rg in md.row_groups() {
            for (leaf, chunk) in rg.columns().iter().enumerate() {
                let root = parquet_schema.get_column_root(leaf).name();
                let Some(c) = columns.iter_mut().find(|c| c.name == root) else { continue };
                c.compressed += chunk.compressed_size().max(0) as u64;
                c.uncompressed += chunk.uncompressed_size().max(0) as u64;
                // GZIP(GzipLevel(6)): the level is the writer's business.
                let codec = chunk.compression().to_string().split('(').next().unwrap_or_default().to_string();
                if !c.codecs.contains(&codec) {
                    c.codecs.push(codec);
                }
            }
        }
        for c in &mut columns {
            let leaves: Vec<_> = parquet_schema.columns().iter().enumerate().filter(|(i, _)| parquet_schema.get_column_root(*i).name() == c.name).map(|(_, d)| d.clone()).collect();
            c.stored = match leaves.as_slice() {
                [one] if one.path().parts().len() == 1 => match one.logical_type_ref() {
                    Some(l) => format!("{} ({l:?})", one.physical_type()),
                    None => one.physical_type().to_string(),
                },
                more => format!("{} leaves", more.len()),
            };
            // Statistics are a leaf's: a nested column has none to give.
            let Ok(conv) = StatisticsConverter::try_new(&c.name, &schema, parquet_schema) else { continue };
            if let Ok(mins) = conv.row_group_mins(md.row_groups()) {
                c.min = extreme(&mins, false);
            }
            if let Ok(maxes) = conv.row_group_maxes(md.row_groups()) {
                c.max = extreme(&maxes, true);
            }
            if let Ok(nulls) = conv.row_group_null_counts(md.row_groups()) {
                let nulls: &UInt64Array = &nulls;
                if nulls.null_count() == 0 {
                    c.nulls = Some(nulls.values().iter().sum());
                }
            }
        }

        let mut codecs: Vec<String> = Vec::new();
        for c in columns.iter().flat_map(|c| &c.codecs) {
            if !codecs.contains(c) {
                codecs.push(c.clone());
            }
        }
        let info = FileInfo {
            rows: fm.num_rows().max(0) as u64,
            row_groups: md.num_row_groups(),
            size: input.len(),
            version: fm.version(),
            created_by: fm.created_by().map(str::to_string),
            keys: fm.key_value_metadata().map(|kv| kv.iter().map(|k| k.key.clone()).collect()).unwrap_or_default(),
            codecs,
        };
        Ok(ParquetFile { input, meta, starts, info, columns, cache: RefCell::new(Cache::default()) })
    }

    pub fn len(&self) -> usize {
        self.starts.last().copied().unwrap_or(0) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The last read that failed, if one did.
    pub fn error(&self) -> Option<String> {
        self.cache.borrow().error.clone()
    }

    /// Rows `start .. start + n`, read from the row groups that hold them.
    pub fn read(&self, start: usize, n: usize) -> Result<Vec<Row>, String> {
        let end = (start + n).min(self.len());
        if start >= end {
            return Ok(Vec::new());
        }
        let group_of = |row: usize| self.starts.partition_point(|&s| s as usize <= row) - 1;
        let (first, last) = (group_of(start), group_of(end - 1));
        let reader = ParquetRecordBatchReaderBuilder::new_with_metadata(self.input.clone(), self.meta.clone())
            .with_row_groups((first..=last).collect())
            .with_offset(start - self.starts[first] as usize)
            .with_limit(end - start)
            .with_batch_size(end - start)
            .build()
            .map_err(err)?;
        let options = FormatOptions::default();
        let mut rows = Vec::with_capacity(end - start);
        for batch in reader {
            let batch = batch.map_err(err)?;
            let formatters = batch.columns().iter().map(|a| ArrayFormatter::try_new(a.as_ref(), &options)).collect::<Result<Vec<_>, _>>().map_err(err)?;
            for i in 0..batch.num_rows() {
                rows.push(formatters.iter().zip(batch.columns()).map(|(f, a)| cell(f, a.as_ref(), i)).collect());
            }
        }
        Ok(rows)
    }

    /// Row `index`, read with the rows about it, kept for when it's shown
    /// again; blank if it couldn't be read ([`ParquetFile::error`] says why).
    pub fn row(&self, index: usize) -> Row {
        self.with_row(index, |r| r.clone())
    }

    /// Whether cell (`row`, `col`) is null.
    pub fn is_null(&self, row: usize, col: usize) -> bool {
        self.with_row(row, |r| r.get(col).is_some_and(Option::is_none))
    }

    /// `f` of row `index`, as [`ParquetFile::row`] reads it, without a copy.
    pub fn with_row<R>(&self, index: usize, f: impl FnOnce(&Row) -> R) -> R {
        let chunk = index / CHUNK;
        let mut cache = self.cache.borrow_mut();
        if !cache.chunks.contains_key(&chunk) {
            let rows = match self.read(chunk * CHUNK, CHUNK) {
                Ok(rows) => {
                    cache.error = None;
                    rows
                }
                Err(e) => {
                    cache.error = Some(e);
                    Vec::new()
                }
            };
            if cache.order.len() >= KEPT {
                if let Some(old) = cache.order.pop_front() {
                    cache.chunks.remove(&old);
                }
            }
            cache.chunks.insert(chunk, rows);
        }
        cache.order.retain(|&c| c != chunk);
        cache.order.push_back(chunk);
        match cache.chunks[&chunk].get(index % CHUNK) {
            Some(row) => f(row),
            None => f(&vec![Some(String::new()); self.columns.len()]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> ParquetFile {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let len = std::fs::metadata(&path).unwrap().len();
        ParquetFile::open(Input::Path(path, len)).unwrap()
    }

    #[test]
    fn every_codec_reads_the_same_rows() {
        let plain = fixture("types-none.parquet");
        for codec in ["snappy", "zstd", "gzip", "lz4", "brotli"] {
            let f = fixture(&format!("types-{codec}.parquet"));
            assert_eq!(f.len(), 1000, "{codec}");
            assert_eq!(f.read(0, 1000).unwrap(), plain.read(0, 1000).unwrap(), "{codec}");
            let named = if codec == "lz4" { "LZ4_RAW".to_string() } else { codec.to_uppercase() };
            assert_eq!(f.info.codecs, [named], "pyarrow's lz4 is LZ4_RAW");
        }
        assert_eq!(plain.info.codecs, ["UNCOMPRESSED"]);
        for c in &plain.columns {
            assert_eq!(c.compressed, c.uncompressed, "{}", c.name);
        }
    }

    #[test]
    fn a_row_is_written_as_its_types_write_it() {
        let f = fixture("types-zstd.parquet");
        let names: Vec<&str> = f.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["id", "name", "score", "ok", "day", "at", "price", "tags", "pos"]);
        let row = f.row(1);
        let cells: Vec<&str> = row.iter().map(|c| c.as_deref().unwrap_or("null")).collect();
        assert_eq!(cells, ["2", "craft-1", "0.5", "false", "2026-01-02", "2026-10-31T12:00:01Z", "0.01", "[t1, u1]", "{x: 1, y: -1}"]);
        assert_eq!(f.row(0)[1], None, "every seventh name is null");
    }

    #[test]
    fn rows_across_row_groups_come_in_order() {
        let f = fixture("types-snappy.parquet");
        assert_eq!(f.info.row_groups, 4);
        let rows = f.read(295, 10).unwrap();
        let ids: Vec<String> = rows.iter().map(|r| r[0].clone().unwrap()).collect();
        assert_eq!(ids, (296..306).map(|i| i.to_string()).collect::<Vec<_>>());
        assert_eq!(f.row(999)[0].as_deref(), Some("1000"), "the last row");
        assert_eq!(f.read(995, 50).unwrap().len(), 5, "cut at the end");
    }

    #[test]
    fn the_footer_gives_each_column_s_range_and_nulls() {
        let f = fixture("types-zstd.parquet");
        let col = |n: &str| f.columns.iter().find(|c| c.name == n).unwrap().clone();
        let id = col("id");
        assert_eq!((id.min.as_deref(), id.max.as_deref(), id.nulls), (Some("1"), Some("1000"), Some(0)));
        assert!(id.numeric && col("price").numeric && !col("name").numeric);
        assert_eq!(col("name").nulls, Some(143));
        assert_eq!(col("day").max.as_deref(), Some("2026-12-31"));
        assert_eq!(col("pos").min, None, "a struct has no statistics of its own");
        assert!(col("tags").kind.starts_with("List("));
        assert!(id.compressed > 0 && id.uncompressed >= id.compressed);
        assert!(f.info.keys.iter().any(|k| k == "ARROW:schema"));
    }

    #[test]
    fn a_file_read_whole_reads_as_one_in_the_project_does() {
        let path = format!("{}/tests/fixtures/types-lz4.parquet", env!("CARGO_MANIFEST_DIR"));
        let whole = ParquetFile::open(Input::Bytes(std::fs::read(&path).unwrap().into())).unwrap();
        assert_eq!(whole.read(290, 20).unwrap(), fixture("types-lz4.parquet").read(290, 20).unwrap());
    }

    #[test]
    fn what_isnt_parquet_says_so() {
        assert!(ParquetFile::open(Input::Bytes(Bytes::from_static(b"name,age\nada,36\n"))).is_err());
    }

    #[test]
    fn sizes_and_counts_read_as_people_say_them() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(34_915), "34.9 KB");
        assert_eq!(bytes(1_200_000_000), "1.2 GB");
        assert_eq!(count(1_000_000), "1,000,000");
        assert_eq!(count(999), "999");
    }
}
