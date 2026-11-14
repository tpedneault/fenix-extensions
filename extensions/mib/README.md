# SCOS-2000 MIB

SCOS-2000 MIBs in Fenix. Open
a MIB -- one in your project is found, nothing to set -- browse it, look a
definition up by kind, write telecommand calls into your
scripts with a form for their arguments, have the MIB check the calls
you wrote, and generate code from it.

- **Open a MIB** (`SPC k o`): every MIB known here -- found in the
  project, named in its settings or yours -- and another folder on disk.
  Picking one opens its page, and it's the MIB `K`, `gd` and the rest use
  in this project from then on.
- **The MIB page** (`SPC k k`). A tab for each kind of definition:
  - `1` telecommands, `2` TC parameters, `3` TM packets, `4` TM
    parameters, `5` calibrations.
  - `/` narrows the list by words and `field:value` filters, and `f`
    opens the filters as a form.
  - `!` lists what's wrong in the MIBs' files, and `6` groups everything
    by PUS service.
  - `Enter` opens a definition's own page, `i` a telecommand's insert
    form.
  - `M` switches to another MIB; the header says where this one is from,
    and how many there are.
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
  between two of the MIBs known here, as Markdown in `generated/`.
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

A MIB is a folder of `.dat` tables -- any of `vdf`, `ccf`, `pcf`, `pid`,
`tpcf`, `caf` or `txf` -- or an XTCE file. The MIBs **known** in a
project are:

1. **Those found in it**, wherever they are: `mib/`, its top, `db/ops/`.
   Nothing to set. The project is looked through as far as 6 folders
   down and 3,000 folders in all, leaving out hidden folders (`.git`),
   `target`, `node_modules`, `generated`, `dist`, `vendor`, virtual
   environments, and the folders its `.gitignore` names. It's looked
   through again when it's been half a minute and the MIBs are needed,
   and every time `SPC k o` or `SPC k r` is used.
2. **Those `mib.roots` names**: the project's own list (in its
   `.fenix/settings.toml`, folders relative to it) in place of yours. In
   a project's list, an XTCE file relative to it is a MIB too
   (`XTCE = "generated/xtce-OPS.xml"`). A setting that names a folder
   the project holds makes it known once, by the setting's name.

The MIBs **in use** -- the MIB page's, `K`, `gd`, completion, the
checks of your scripts, the insert form, the pickers, the notebook's
`mib:` links and agents' `mib_lookup` -- are:

1. the one you opened with `SPC k o` in this project, kept for it from
   one session to the next (in the extension's own folder: never in the
   project's settings, and `mib.default` is left as it is);
2. else every MIB the settings name, together, as before -- `mib.default`
   says whose definition wins a name several define;
3. else the first found: `mib/`, the project's top, then the shallowest.

A project with one MIB anywhere in it just works: `SPC k k` shows it.

`SPC k o` lists every known MIB -- found ones by their path in the
project, then the project's settings', then yours -- each with how many
tables it has and whether it has telecommands and TM, `●` on the one in
use. With several in the settings, "all of them together" goes back to
using them together. **Another folder…** opens a MIB elsewhere: type or
paste its full path (the folder selected in the explorer is filled in).
It's added to *your* `mib.roots` for you -- named after its folder, made
unique (`OPS`, `OPS-2`), never twice -- and picked here. Fenix gives the
extension the folders `mib.roots` names as it starts it, so a folder
just added opens the next time Fenix starts, by itself. In a project
with its own `mib.roots`, which takes the place of yours there, a folder
elsewhere goes in the project's list: `SPC k o` opens its settings
rather than writing them.

What's generated (`SPC k g`) and compared (`SPC k D`) goes in the
project's `generated/`, wherever the MIB is in it; the doctor counts the
project's `recordings/`.

They're read when first needed, and again when one of their files
changes (`mib.watch`) or a MIB folder appears in the project or goes.

The mission profile they're read and packets built against is the
CCSDS settings (`ccsds.pus`, `ccsds.tm_time`, ...). The CCSDS extension
keeps those; the MIB reads them.

## Keys

| Key | Command | What it does |
| --- | --- | --- |
| `SPC k k` | `mib.page` | The MIB page |
| `SPC k o` | `mib.open` | Open a MIB: found here, named in the settings, or another folder |
| `SPC k /` | `mib.search` | Every kind of definition, by name or description |
| `SPC k t` `p` `m` `n` `c` | `mib.telecommand`, `mib.tc_parameter`, `mib.tm_packet`, `mib.tm_parameter`, `mib.calibration` | One kind, by name |
| `SPC k i` | `mib.insert` | A telecommand, then its insert form |
| `SPC k e` | `mib.edit_call` | The call on this line, in the insert form |
| `SPC k b` | `mib.copy_bytes` | The call on this line, as its packet's bytes |
| `SPC k g` | `mib.generate` | Generate from the MIB |
| `SPC k q` | `mib.insert_sequence` | A command sequence, as the telecommands it sends |
| `SPC k D` | `mib.diff` | Two MIBs known here, compared |
| `SPC k r` | `mib.reload` | Read the MIBs again |
| `SPC k ,` | `mib.settings` | This project's settings |

What it says when it names a key -- a card, the doctor, a missing MIB --
gives the keys you have: `C-k k k` with standard keys where this table
says `SPC k k`.

In a Tcl file, `SPC m` has `i`, `e`, `q`, `t`, `k`, `p`, `c`, `m`, `o` and `r`,
beside the Tcl extension's keys.

## The project doctor

When the MIBs in use are the project's -- found in it, wherever, or
named by its own settings -- the doctor lists:

- each one's tables and rows, where it is, and their layout (ICD issue);
- the rows with problems;
- tables the ICD doesn't know;
- the mission profile, and whether the MIBs agree with it and the
  standards;
- the project's recordings;
- the project's other MIBs, which `SPC k o` opens.

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
- Read the project (to find its MIBs), and write what it generates into
  its `generated/`.
- Read and change the MIB folders `mib.roots` names, and add a folder to
  your `mib.roots` when you open one with "another folder…".
- Read the CCSDS settings.
