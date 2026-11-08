# SCOS-2000 MIB 1.2.0 and CCSDS 1.2.0: the release

What ships, what has been checked and what still needs checking, and the
steps to put both in the registry.

## What ships

| Extension | From | To | Why |
|---|---|---|---|
| `fenix.mib` | 1.1.0 | 1.2.0 | Telecommand reading per the ICD, repeated groups, value sets, PTV, verification, sequences, MIB diff, VPD/CUR, XTCE groups ([CHANGELOG](../extensions/mib/CHANGELOG.md)) |
| `fenix.ccsds` | 1.1.0 | 1.2.0 | Built on the same `crates/mib`: grouped TCs decode right, VPD packets, CUR calibrations, the `layout` check ([CHANGELOG](../extensions/ccsds/CHANGELOG.md)) |
| `fenix.mission-bundle` | 1.0.0 | unchanged | Its dependencies are `^1`: it picks the new versions up |

`crates/mib` (shared, unpublished) goes from 1.0.0 to 1.1.0. Both
extensions keep `fenix = ">=1.1, <2"` and API 1.0: no host call is new.
They must be released together, from the same commit. If only one is
released, the two extensions read the same MIB differently.

## Checked

- **Unit and integration tests:**
  - `crates/mib`: 106 tests, plus `tests/realistic.rs`.
  - `crates/ccsds`: 34 tests.
  - `extensions/mib`: 32 tests.
  - `extensions/ccsds`: 21 tests.
  - `xtask`: 6 tests.
- **Clippy:** `-D warnings` is clean on every crate touched.
- **Manifests and packs:** `cargo xtask validate` passes. Both extensions build for `wasm32-wasip2`, and `cargo xtask pack` packs both.
- **End to end** (`crates/mib/tests/realistic.rs`): a mission-like MIB covering:
  - PUS 3 with nested groups, and PUS 20 with a deduced value.
  - CDF defaults, hex radix, a raw range behind a curve, a float, a time, a variable-length string and octets.
  - Value sets, PTV, verification stages, a TCP/PCDF header, a sequence, and a Latin-1 `cdf.dat`.

  Every telecommand is filled the way the form fills it, then built, decoded, identified, checked with the MIB checks (no findings), exported to XTCE, read back, and built again to the same bytes. The test found and fixed four bugs (see the commit "an end-to-end test of a mission-like MIB").
- **In Fenix's wasm host** (headless, `fenix-wasm`'s `Worker`, the real `extension.toml`'s permissions): the packed `extension.wasm` passed 18 of 18 checks.
  - The insert form: value set HK_DEF, `+` growing a group, Bytes with each value at its octet, and `I` inserting.
  - The heater's PTV note and CDF default.
  - The MIB page.
  - Definition pages with verification, value sets and the packet header.
  - A sequence inserted as its telecommand.
  - XTCE generated into `generated/`.

  Not covered there: the language server's diagnostics and `K` (the harness doesn't drive LSP; `mib_check` and `hover_text` have unit tests). Packages are rebuilt by `release.yml` from the tag, so a local `extension.wasm` doesn't matter. Repack before any manual install.
- **Live in Fenix 1.1.0 on Windows** (2026-11-08): both packs installed from disk into a clean profile, driven by keyboard, with a test MIB written for it (two ICD 7.x roots, OPS and a changed copy OPS2, 8 telecommands, 2 TM packets) and a Tcl script. Passed:
  - `SPC k k`, `!`: only the expected problems (the Latin-1 `cdf.dat` with its line, and the broken TC's `layout` findings, in both MIBs).
  - `SPC k i`: every `E` element with its default -- `CDF_VALUE` before `CPC_DEFVAL`, raw, hex (`CPC_RADIX` H), engineering through a curve, a status text, `CPC_DEFVAL` alone. Range checks: raw `PRF` behind a curve, one-value `PRV` with a second range, engineering range, float range.
  - Groups: E/A/F in a group with an element after it, numbering (`PN · 2 of 3`, `PPAR[1.2]`), `+`/`-` from inside, `h`/`l`, zero repetitions (what follows moves back by the group), nested PUS 3,1, a fixed-count repeater, PUS 20,3 deduced values (16-bit then 8-bit from `PCF_PID`). Bytes hand-checked against the CDF for each.
  - Float, CUC time, relative time, variable string (length octet, what follows moved), octets. `v` value sets (`CCF_DEFSET` first), `R`, the PTV note, no false `PCDF` mismatch.
  - `I`, `SPC k e` (nested call read back), `SPC k b` + `SPC k d`, `D`: identified with the typed values. Definition page sections. `SPC k q` (formal's default, timing comments). `SPC k D` (only the two changes). VPD packet and `CUR` calibration selection on hand-built TM.
  - Script checks: missing repetition, one too many, out of range (int, float), fixed-count repetition missing, defaulted argument missing, PTV info, `K` with nested arguments.
  - The XTCE export read back as a MIB root: no problems of its own, and the same bytes from the form (defaults, groups).

  It found and fixed (commit "Fixes from the live test of 1.2.0"): a fixed-count group's headings (`1 of 0`); `-` dropping out of the group it was shrinking; deduced values not checked (`300` for an 8-bit parameter built silently as `0x2C`); TC time arguments decoded as octets; several MIBs generated into one file (every XTCE name twice); test vectors failing on octets and PUS 20 with no default; an XTCE file never accepted as a MIB root (the host mounts folders only); the XTCE round trip losing PI1/PI2 (two HK packets no longer told apart) and reading `initialValue` in the MIB's radix (`ABCD` came back as `0x43981`); `SPC k d` on a `tc_vectors.txt` line taking the mnemonic's digits for hex.

## Still to check before tagging

1. **One of your real mission MIBs**, in the GUI. The ICD text wasn't available while this was built, nor for the live test. These readings come from the field names and the ICD's change notes, and should be confirmed against your copy of the ICD (EGOS-MCS-S2K-ICD-0001) or a MIB SCOS accepts. The live test exercised flat nested-group counting, positions after a group, value-set links, a sequence with a formal parameter, variable-length strings and `VPD` groups, and found them consistent end to end (form, checks, packet, decoder, XTCE) -- which shows Fenix agrees with itself, not that it agrees with the ICD. Not exercised: `CDF_INTER` `D`/`T`, `SDF_REPPOS`, `VPD_CHOICE`/`VPD_PIDREF`. Also confirm `CCF_NPARS` counts `E` and `F` elements but not `A` ones, as the `layout` check does.
   - **Nested groups:** `CDF_GRPSIZE` counts a nested group's elements in the outer group's size (flat counting). `VPD_GRPSIZE` is read the same way.
   - **Positions after a group:** offsets are computed as if the group were repeated once. This one the ICD does say.
   - **`CDF_INTER` `D`/`T`:** a dynamic default, taken from `CDF_TMID`.
   - **Default interpretation:** an empty `*_INTER` means raw. A value that isn't a raw number, such as a status text, is read as engineering.
   - **Value sets:** `PSV_PVSID` ↔ `PVS_ID`, `PVS_PSID` ↔ `PST_NAME`, and `CCF_DEFSET` naming a `PSV_NAME`.
   - **`SDF_REPPOS`:** the occurrence of a parameter among a command's expanded fields.
   - **Variable-length octets and strings:** a one-octet length, then the data.
   - **`VPD_CHOICE` / `VPD_PIDREF`:** `CHOICE` gives a following deduced entry its type. `PIDREF` embeds the packet its value names.
   - **`PCDF`:** shown, and only *checked* against the built packet. Packets are still built from the mission profile.

   Run in a project that holds the MIB:
   - `SPC k k`, then `!`. Look at new problems from the `layout` check. Expect some: `CCF_NPARS` mismatches and groups running past their elements are real findings. If a finding is wrong, it can be turned off with `ccsds.checks = ["layout"]`.
   - `SPC k i` on a TC with a group, and on one whose arguments have `CDF_VALUE` defaults: the fields, the `+`/`-` keys, `v`, and `b`.
   - `SPC k b` on existing script lines, then `D` to decode: the packet should match what your ground system builds for the same call.
   - `SPC k g`, `xtce.xml`, into your XTCE consumer, if one reads Fenix's XTCE.
2. **Scripts:** look at the diagnostics on a few real procedures. Calls that left out an argument with a `CDF_VALUE` default are now flagged as missing, which is correct but new.

## Behaviour people will notice

- More arguments in the form and in the checks: every `E` element with a `CDF_VALUE` default.
- Packets built for TCs with groups or defaults change. The old bytes were wrong.
- New MIB problems from the `layout` check.
- The XTCE export has a `CommandContainerSet` for groups and `AncillaryData` (`SCOS_PTC`, `SCOS_PFC`, `SCOS_CATEG`, `SCOS_RADIX`, `SCOS_PID`) on types and parameters. Both are valid XTCE 1.1 and 1.2.
- Form values remembered from 1.1 keep their keys (`NAME@1.2`) and carry over.

## Steps

1. Open a PR from `claude/scos-2000-mib-review-l4o34m` to `main`. CI (`ci.yml`) runs clippy, every crate's tests, validate and pack.
2. Do the real-MIB checks above on the PR's build. Install it with `cargo xtask pack extensions/mib dist`, then install the `.fenixext` from disk.
3. Merge to `main`.
4. Tag the merge commit, CCSDS first and MIB second, both on the same commit:
   ```sh
   git tag ccsds-v1.2.0 <merge-sha> && git push origin ccsds-v1.2.0
   git tag mib-v1.2.0   <merge-sha> && git push origin mib-v1.2.0
   ```
   `release.yml` packs each one, attaches it with its SHA-256 to a GitHub release (the notes are its CHANGELOG), and adds it to the signed `index.json` on `registry`. The `registry` concurrency group runs them one at a time.
5. Check the release:
   - Both GitHub releases have their `.fenixext` and `.sha256`.
   - The `registry` branch's `index.json` lists `fenix.mib` 1.2.0 and `fenix.ccsds` 1.2.0.
   - `index.json.minisig` was updated.
   - In Fenix 1.1 and 1.2, `SPC h x` offers the update. After updating, `SPC k k` and `SPC k i` work on the dev mission (`fenix/dev/ccsds-sim/mission`).
6. If something breaks after release, ship 1.2.1 for both from a fix branch. The index takes the newer version. Pulling a release out of the index is a manual edit of the `registry` branch, re-signed with the registry key.

## Afterwards

From the live test (2026-11-08), not fixed:
- XTCE export writes no argument ranges (`ValidRange`), and import keeps one range: a MIB read back from its XTCE builds the same bytes but checks no ranges. Older than 1.2.
- A hex-radix default is offered and inserted in decimal (`ABCD` as `43981`). Correct, but a mission used to hex may want `0xABCD`.
- Completion isn't call-aware: after `-P` in a call it lists every TC parameter, not that telecommand's.
- The `layout` problems point at the `ccf.dat` row, not the `cdf.dat` row that's wrong.
- Fenix (host): project settings aren't re-read when `.fenix/settings.toml` changes (`SPC k r` reads the old `mib.roots`); 1.1.0 stalled once on its splash screen at start.

- Building packets from a `PCDF` header (instead of the profile's), behind a setting. This is for missions whose TC header isn't PUS.
- A form for a sequence's formal parameters (today: their defaults, as comments).
- `SDF_VALSET` (a step's value set) applied to the step.
- Dynamic-length arrays on the TM side of XTCE import.
