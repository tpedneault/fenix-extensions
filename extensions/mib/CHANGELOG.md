# Changelog

## 1.3.0

- For Fenix 1.5 (extension API 1.5): a name's card says the keys of the
  key scheme in use for going to its page -- `F12` with standard keys,
  `gd` with Vim keys. The README gives standard keys beside Vim's.

## 1.2.0

Telecommands are read the way the SCOS-2000 Database Import ICD lays
them out, and the insert form, the scripts' checks and the packets
built from them follow it.

- **Every parameter is asked for.** Whether an element is fixed is
  `CDF_ELTYPE`'s (`A` area, `F` fixed, `E` editable): an editable
  parameter with a default in `CDF_VALUE` used to be taken for a fixed
  one, left out of the form, the checks, completion, `mib_lookup` and
  the generators, and built into the packet with its default whatever
  was typed. `CDF_VALUE` is now its default, before `CPC_DEFVAL`, read
  raw or engineering as `CDF_INTER` / `CPC_INTER` say; a dynamic
  default names the TM parameter it comes from.
- **Repeated groups.** A counter (`CDF_GRPSIZE`) repeats every element
  after it -- fixed and spare ones and nested groups included, as the
  ICD counts them -- so what follows a group is no longer pulled into
  it, the form and the packet agree, and elements are in `CDF_BIT`
  order. After a group, positions move by its extra repetitions (or
  back by its length when it's repeated no times); packets with groups
  decode with each repetition where it is, and identify by the fixed
  values before the first group.
- **The form**: repetitions numbered (`PT[2]`, `PN · 2 of 3`), `+`/`-`
  change the counter of the group you're in, `v` fills it from the
  MIB's value sets (`CCF_DEFSET` first; `PST`/`PSV`/`PVS`/`PSM`), a
  telecommand with pre-transmission validation (`PTV`) says what it
  waits for, and Bytes lists where each value went and where the
  packet disagrees with the MIB's `TCP`/`PCDF` header.
- **Values**: raw values in `CPC_RADIX`'s radix (hex without `0x`),
  ranges in raw or engineering units as `PRF_INTER` says, `PRV` rows
  that allow a single value, floats checked as numbers, octets, strings
  (variable-length ones with their length), absolute and relative
  times, and deduced parameters (`PTC 11`) typed by the TM parameter an
  earlier argument names (`CPC_CATEG` `P`).
- **The scripts' checks** follow the counters: a missing repetition,
  one value too many, a value given twice; a deduced value is checked as
  the parameter it names (the form shows that type too); and a note on a
  telecommand sent only while the TM shows something.
- **The MIB's own checks** (`layout`, in `ccsds.checks`): `CCF_NPARS`
  against `cdf.dat`, a group running past its elements, a counter that
  isn't an unsigned integer, a fixed element with no value, a default
  or fixed value that doesn't encode or is outside its range, an
  element's width against its type; overlaps on telecommands with
  groups too.
- **A telecommand's page and `K`**: group members under their counter,
  pre-transmission validation, value sets, verification stages
  (`CVP`/`CVS`/`CVE`, or the reports the acknowledgement flags ask for)
  and its packet header. `mib_lookup` answers them too.
- **Command sequences** (`SPC k q`): a sequence inserted as the
  telecommands it sends, through the project's templates, with its
  formal parameters' defaults and its timing as comments.
- **Two MIBs compared** (`SPC k D`): what was added, removed and
  changed between two of the project's MIBs, into `generated/`.
- **Variable TM packets** (`VPD`) and **calibration selection**
  (`CUR`) when packets are decoded.
- **XTCE**: groups export as repeated command containers and import
  back (and arrays whose length an argument gives); a MIB's own type
  codes, categories, radix and parameter ids go as ancillary data, so
  a MIB exported and read back builds the same packets, offers the same
  defaults (an `initialValue` is no longer read in the MIB's radix) and
  tells packets apart by their PI1/PI2 values as the MIB does. The
  Python, Wireshark, C and ICD generators know groups. With several MIBs,
  `SPC k g` generates from one of them, into its own file
  (`xtce-OPS.xml`): they used to go into one file, every name twice. An
  XTCE file in the project can be one of its MIBs (`mib.roots`), to read
  an export back. Test vectors build telecommands with octets or a
  parameter id and no default.
- A `.dat` file that isn't UTF-8 is read as Latin-1, with a problem
  saying where, instead of losing the whole table; `mib.columns` may
  name the TC parameter column `CPC_PNAME`, as the ICD does.

## 1.1.0

For Fenix 1.1: `mib_lookup`, a tool for your coding agents (read). It
finds a definition by name -- a mnemonic, a SPID, a calibration's id --
or by the words of its description, as `/` on the MIB page does, and
answers what its page shows: a telecommand's arguments with their bits,
types, defaults, allowed values and calibrations; a parameter's type,
calibration, limits and the packets carrying it; a packet's parameters
by position; a calibration's points or texts. Other matches are listed.

## 1.0.0

- The first release, for Fenix 1.0: sCOS-2000 MIBs: browse them, telecommands and parameters in your scripts (K, gd, completion, checks), the telecommand insert form, and XTCE, C, Python or a Wireshark dissector generated from them.
