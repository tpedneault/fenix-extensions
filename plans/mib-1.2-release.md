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
  - `crates/mib`: 104 tests, plus `tests/realistic.rs`.
  - `crates/ccsds`: 34 tests.
  - `extensions/mib`: 29 tests.
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

## Still to check before tagging

1. **One of your real mission MIBs**, in the GUI. The ICD text wasn't available while this was built. These readings come from the field names and the ICD's change notes, and should be confirmed against your copy of the ICD (EGOS-MCS-S2K-ICD-0001) or a MIB SCOS accepts:
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

- Building packets from a `PCDF` header (instead of the profile's), behind a setting. This is for missions whose TC header isn't PUS.
- A form for a sequence's formal parameters (today: their defaults, as comments).
- `SDF_VALSET` (a step's value set) applied to the step.
- Dynamic-length arrays on the TM side of XTCE import.
