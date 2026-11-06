# Changelog

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
