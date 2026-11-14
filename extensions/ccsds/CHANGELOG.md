# Changelog

## 1.4.0

- **Packets are named through the MIB you have open.** The MIBs the CCSDS
  extension decodes against are the ones the MIB extension (1.5) has in
  use: the one picked with `SPC k o` in the project, the MIBs your
  settings name, or the one found anywhere in the project -- where 1.3
  looked only at `mib.roots`, then the project's `mib/` or its top. When
  the pick changes, the CCSDS extension follows. Without the MIB
  extension it follows the same rule, without a pick.

## 1.3.0

- For Fenix 1.5 (extension API 1.5): what it says names the keys of the
  key scheme in use, and yours where you rebound them -- `K` on hex or an
  on-board time ends "`C-k k d` opens the decode" or "`C-k k T` converts
  it" with standard keys and `SPC k d`, `SPC k T` with Vim keys; so do
  the standards page and its messages (the explorer, a recording, a
  live source, the standards, the Extensions page for the PDF reader).
  The MIB's settings are read as before.

## 1.2.2

- A command that reads a large MIB may take up to a minute, rather than
  being stopped after 5 seconds.

## 1.2.1

- A large MIB takes about a third of the memory it did (the SCOS-2000
  MIB extension's 1.2.1 reading of it), so the extension is no longer
  stopped for running out of memory reading one.

## 1.2.0

The MIB is read with the SCOS-2000 MIB extension's 1.2 reading of it:

- Telecommands with repeated groups decode with each repetition where
  it is; one whose editable argument has a default in `CDF_VALUE` is
  still identified when another value was sent.
- Variable TM packets (`VPD`) decode entry by entry, their groups
  repeated as their counters say; a parameter's calibration is chosen
  by its `CUR` rows when it has some.
- Telecommand arguments are shown as their status texts and through
  their numeric calibrations; times as the UTC time or duration they
  are.
- A new MIB check, `layout` (`ccsds.checks`): telecommand definitions
  the ICD's rules disagree with.
- A MIB table that isn't UTF-8 is read as Latin-1 instead of being lost.
- An XTCE file in the project can be one of its MIBs (`mib.roots`).
- `SPC k d` on a line that starts with a name ending in digits
  (`TTYP0001 18 64 ...`, a line of the MIB's `tc_vectors.txt`) no longer
  takes the name's digits for hex.

## 1.1.0

For Fenix 1.1: `ccsds_decode`, a tool for your coding agents (read).
Given bytes as hex, it decodes them as the packet inspector does --
a space packet through the MIB, a transfer frame or a CLTU with the
packets it carries, a CLCW, an on-board time, a CFDP PDU -- against the
project's mission profile, and answers the whole tree as JSON: each
field's offset, raw value, meaning and check, the definition the MIB
identified, and the checks that fail.

## 1.0.0

- The first release, for Fenix 1.0: space packets, transfer frames and CLTUs decoded through the MIB, recordings and live telemetry with a parameter followed through them, on-board times, hex, and the CCSDS and ECSS standards.
