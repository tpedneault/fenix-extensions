# The test files, written by pyarrow (libzstd's zstd, not ruzstd's): one
# per codec, 1,000 rows in row groups of 300, small pages, a page index.
#
#   python tests/fixtures.py tests/fixtures
import sys, datetime, decimal
import pyarrow as pa, pyarrow.parquet as pq
out = sys.argv[1]
n = 1000
t = pa.table({
    "id": pa.array(range(1, n + 1), pa.int64()),
    "name": pa.array([None if i % 7 == 0 else f"craft-{i}" for i in range(n)], pa.string()),
    "score": pa.array([i * 0.5 for i in range(n)], pa.float64()),
    "ok": pa.array([i % 2 == 0 for i in range(n)], pa.bool_()),
    "day": pa.array([datetime.date(2026, 1, 1) + datetime.timedelta(days=i % 365) for i in range(n)], pa.date32()),
    "at": pa.array([datetime.datetime(2026, 10, 31, 12, 0, 0) + datetime.timedelta(seconds=i) for i in range(n)], pa.timestamp("us", tz="UTC")),
    "price": pa.array([decimal.Decimal(i) / 100 for i in range(n)], pa.decimal128(10, 2)),
    "tags": pa.array([[f"t{i % 3}", f"u{i % 5}"] if i % 4 else [] for i in range(n)], pa.list_(pa.string())),
    "pos": pa.array([{"x": i, "y": -i} for i in range(n)], pa.struct([("x", pa.int32()), ("y", pa.int32())])),
})
for codec in ["snappy", "zstd", "gzip", "lz4", "brotli", "none"]:
    pq.write_table(t, f"{out}/types-{codec}.parquet", compression=codec, row_group_size=300, data_page_size=1024, write_page_index=True)
