# Changelog

## 1.0.1

- A file in the open project is read where it is, whatever its size: 1.0.0 couldn't tell which project was open (it lacked `editor = "read"`), so it read every file whole and refused one over 64 MiB.
- Why a file wasn't read is wrapped onto the lines below, not cut off after a long path.

## 1.0.0

- The first release, for Fenix 1.0: Parquet files read in a pane -- their rows as a table, read a page at a time, and their schema with each column's type, nulls, range, size and codec.
