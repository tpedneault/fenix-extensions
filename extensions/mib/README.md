# SCOS-2000 MIB

SCOS-2000 MIBs in Fenix. Browse
a MIB, look a definition up by kind, write telecommand calls into your
scripts with a form for their arguments, have the MIB check the calls
you wrote, and generate code from it.

- **The MIB page** (`SPC k k`). A tab for each kind of definition:
  - `1` telecommands, `2` TC parameters, `3` TM packets, `4` TM
    parameters, `5` calibrations.
  - `/` narrows the list by words and `field:value` filters, and `f`
    opens the filters as a form.
  - `!` lists what's wrong in the MIBs' files, and `6` groups everything
    by PUS service.
  - `Enter` opens a definition's own page, `i` a telecommand's insert
    form.
- **A definition's page.** Every name on it is a link: `Enter` follows
  it, and `H`/`L` (or Backspace) go back and forward.
- **The insert form** (`SPC k i`, or `i` on a telecommand).
  - Every argument at once (`CDF_ELTYPE` `E`), each with its default
    (`CDF_VALUE`, else `CPC_DEFVAL`), type, unit and what it may be.
  - A counter (`CDF_GRPSIZE`) repeats the elements after it as often as
    its value says, each repetition numbered (`PT[2]`); `+`/`-` change
    the counter of the group the cursor is in.
  - `v` fills the form from the MIB's value sets, its default set
    first; a telecommand sent only while the TM shows something
    (`PTV`) says so.
  - The command is written with the project's templates, and a value
    outside what the MIB allows is flagged beside it.
  - `I` inserts it where your cursor was. `SPC k e` reads the call on
    the cursor's line back into the form, and inserting puts it back in
    its place.
  - `b` shows the packet's bytes and where each value went, `y` copies
    them and `D` decodes them (with the CCSDS extension).
- **In your scripts**, through a language server:
  - `K` on a MIB name shows its card, and `gd` opens its page (with
    standard keys, `Ctrl+K Ctrl+I` and `F12`, or Ctrl+click).
  - Completion offers telecommands and parameters.
  - Telecommand calls the MIB disagrees with are underlined: unknown
    mnemonics, missing or out-of-range arguments, and repetitions that
    don't match their counter (`mib.check_scripts`).
  - `mib.editor_files` narrows which files; none listed means every
    file.
- **Command sequences** (`SPC k q`): a sequence inserted as the
  telecommands it sends, with its formal parameters' defaults, its
  timing and comments as comments.
- **Two MIBs compared** (`SPC k D`): what was added, removed and changed
  between two of the project's MIBs, as Markdown in `generated/`.
- **Generated from the MIB** (`SPC k g`): XTCE, a Wireshark dissector, C,
  Python, an ICD, or TC test vectors, written to the project's
  `generated/` -- repeated groups included.

## How telecommands are read

As the SCOS-2000 Database Import ICD lays them out:

- `CDF_ELTYPE` says what's asked: `E` elements are, `F` (a fixed
  parameter) and `A` (a fixed area) aren't. An `E` element's
  `CDF_VALUE` is its default, read raw or engineering as `CDF_INTER`
  says; `D`/`T` take it from the TM parameter `CDF_TMID` when it's sent.
- A counter's `CDF_GRPSIZE` counts every element after it in its group
  -- fixed and spare ones, and a nested group's elements too.
- `CDF_BIT` places each element as if every group were there once:
  with more repetitions, what follows moves by them; with none, back by
  the group's length. A variable-length value moves what follows by how
  much longer it is than `CDF_ELLEN`.
- Raw values are in `CPC_RADIX`'s radix; ranges in raw or engineering
  units as `PRF_INTER` says; a `PRV` row with no maximum allows one
  value. A deduced parameter (`PTC 11`) takes the type of the TM
  parameter an earlier argument (`CPC_CATEG` `P`) names.

## Which MIBs

1. The folders `mib.roots` names: the project's own list (in its
   `.fenix/settings.toml`, folders relative to it) in place of yours.
   In a project's list, an XTCE file relative to it is a MIB too
   (`XTCE = "generated/xtce-OPS.xml"`).
2. Else the project itself, when it's a MIB (`.dat` tables in it or in
   its `mib/`).

They're read when first needed, and again when one of their files
changes (`mib.watch`).

The mission profile they're read and packets built against is the
CCSDS settings (`ccsds.pus`, `ccsds.tm_time`, ...). The CCSDS extension
keeps those; the MIB reads them.

## Keys

| Key | Command | What it does |
| --- | --- | --- |
| `SPC k k` | `mib.page` | The MIB page |
| `SPC k /` | `mib.search` | Every kind of definition, by name or description |
| `SPC k t` `p` `m` `n` `c` | `mib.telecommand`, `mib.tc_parameter`, `mib.tm_packet`, `mib.tm_parameter`, `mib.calibration` | One kind, by name |
| `SPC k i` | `mib.insert` | A telecommand, then its insert form |
| `SPC k e` | `mib.edit_call` | The call on this line, in the insert form |
| `SPC k b` | `mib.copy_bytes` | The call on this line, as its packet's bytes |
| `SPC k g` | `mib.generate` | Generate from the MIB |
| `SPC k q` | `mib.insert_sequence` | A command sequence, as the telecommands it sends |
| `SPC k D` | `mib.diff` | Two of the project's MIBs, compared |
| `SPC k r` | `mib.reload` | Read the MIBs again |
| `SPC k ,` | `mib.settings` | This project's settings |

What it says when it names a key -- a card, the doctor, a missing MIB --
gives the keys you have: `C-k k k` with standard keys where this table
says `SPC k k`.

In a Tcl file, `SPC m` has `i`, `e`, `q`, `t`, `k`, `p`, `c`, `m` and `r`,
beside the Tcl extension's keys.

## The project doctor

In a project that's a MIB, the doctor lists:

- its tables and rows, and their layout (ICD issue);
- the rows with problems;
- tables the ICD doesn't know;
- the mission profile, and whether the MIB agrees with it and the
  standards;
- its recordings.

## For coding agents

`mib_lookup` (read, Fenix 1.1) gives your coding agents the MIBs here:
a definition by name -- a telecommand's mnemonic, a TM packet's SPID or
name, a calibration's id -- or by the words of its description, as `/`
on the MIB page finds them (`kind` narrows it to one kind). It answers
what the definition's page shows, as JSON: a telecommand's PUS type,
APID and arguments, each with its kind (editable, fixed, area), bits,
type, default, what it may be, its calibration and the groups it
repeats in, and its value sets, pre-transmission validation and
verification stages; a parameter's type, unit, calibration, limits and
the packets that carry it; a packet's parameters by position; a
calibration's points, texts, ranges or coefficients. When words match
several, the best is answered in full and the next twenty are listed.

## What it may do

- Know where you are, and type into the file you're in (the insert form,
  `SPC k e`).
- Read the project, and write what it generates into its `generated/`.
- Read and change the MIB folders `mib.roots` names.
- Read the CCSDS settings.
