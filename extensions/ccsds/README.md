# CCSDS & ECSS

Space packets, transfer frames and CLTUs in Fenix.

- **Decode** (`SPC k d`). The hex under the cursor, selected or on the
  clipboard, as a tree:
  - a space packet, through the MIB;
  - a TM, AOS or USLP frame with its packets, Reed-Solomon corrected and
    de-randomized as the mission says;
  - a CLTU with its TC frame;
  - a CLCW, an on-board time, or a CFDP PDU.

  Every field has its raw bytes, value and check. `a` reads the bytes
  as something else, and a field names what's behind it: a MIB
  definition (with the MIB extension) or a standard's heading.
- **Recordings** (`SPC k f`). A file of packets, frames, CLTUs or records,
  or a text log of hex lines, as a list of packets:
  - each one named through the MIB, with lost frames and bad packets;
  - by APID;
  - a TM parameter followed through them (`t`), with its curve and its
    limit crossings;
  - what's wrong.

  `w` writes the filtered packets or the parameter's samples.
- **Live sources** (`SPC k l`). The same list, filled as telemetry
  arrives from a source `ccsds.sources` lists:
  - `tcp://host:port`, `udp://:port`;
  - `nats://host:port` with a subject;
  - `file://path`, a recording still being written, in the project.

  A followed parameter says when it crosses a limit.
- **The time converter** (`SPC k T`): an on-board time code, UTC, TAI,
  GPS, CDS, from the mission's epoch and its time correlation.
- **Hex view** (`SPC f x`), and Fenix shows a binary file with it.
- **The standards** (`SPC k ?`): the CCSDS and ECSS standards, which of
  them the `ccsds.library` folder has, and a search through all of them
  (with the PDF extension).
- **`K`** on hex or an on-board time, in any file: what it is, in short.
- **The mission clock** (`ccsds.clock`): the time now as the mission
  writes it on board, in the modeline.

## The mission

How the mission's bytes are laid out is its `ccsds.*` settings:
- the PUS edition, time format, epoch and time correlation, checksum;
- its frames and TC frames;
- its virtual channels' names, live sources and checks.

A project's own settings come first. Packets are named through the MIBs
the MIB extension reads (`mib.roots`).

## Keys

| Key | Command | What it does |
| --- | --- | --- |
| `SPC k d` | `ccsds.decode` | Decode the hex here |
| `SPC k f` | `ccsds.recording` | Open the recording selected in the explorer, or the file you're in |
| `SPC k l` | `ccsds.live` | Follow a live source |
| `SPC k T` | `ccsds.time` | The time converter |
| `SPC k ?` | `ccsds.standards` | The standards |
| `SPC f x` | `ccsds.hex` | The file as hex |

## What it may do

- **Where you are:** know where you are, and type into the file you're in
  (the time converter's `i`).
- **Files:** read the project, and write into it what a stream page
  writes.
- **Network:** connect to the live sources `ccsds.sources` lists. It
  only receives.
- **Your folders and settings:** read the MIB folders, the standards
  folder and the MIB settings.
- **Clipboard:** read the clipboard (`SPC k d` on copied hex).
