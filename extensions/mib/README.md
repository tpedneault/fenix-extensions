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
  - Every argument at once, each with its default, type, unit and what
    it may be.
  - The command is written with the project's templates, and a value
    outside what the MIB allows is flagged beside it.
  - `I` inserts it where your cursor was. `SPC k e` reads the call on
    the cursor's line back into the form, and inserting puts it back in
    its place.
  - `b` shows the packet's bytes, `y` copies them and `D` decodes them
    (with the CCSDS extension).
- **In your scripts**, through a language server:
  - `K` on a MIB name shows its card, and `gd` opens its page.
  - Completion offers telecommands and parameters.
  - Telecommand calls the MIB disagrees with are underlined: unknown
    mnemonics, and missing or out-of-range arguments
    (`mib.check_scripts`).
  - `mib.editor_files` narrows which files; none listed means every
    file.
- **Generated from the MIB** (`SPC k g`): XTCE, a Wireshark dissector, C,
  Python, an ICD, or TC test vectors, written to the project's
  `generated/`.

## Which MIBs

1. The folders `mib.roots` names: the project's own list (in its
   `.fenix/settings.toml`, folders relative to it) in place of yours.
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
| `SPC k r` | `mib.reload` | Read the MIBs again |
| `SPC k ,` | `mib.settings` | This project's settings |

In a Tcl file, `SPC m` has `i`, `e`, `t`, `k`, `p`, `c`, `m` and `r`,
beside the Tcl extension's keys.

## The project doctor

In a project that's a MIB, the doctor lists:

- its tables and rows, and their layout (ICD issue);
- the rows with problems;
- tables the ICD doesn't know;
- the mission profile, and whether the MIB agrees with it and the
  standards;
- its recordings.

## What it may do

- Know where you are, and type into the file you're in (the insert form,
  `SPC k e`).
- Read the project, and write what it generates into its `generated/`.
- Read and change the MIB folders `mib.roots` names.
- Read the CCSDS settings.
