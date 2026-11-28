+++
id = "ADR-001"
type = "decision"
title = "Store readings in SQLite"
status = "accepted"
+++

## Context

Readings must survive a power cut.

## Decision

SQLite in WAL mode.

## Consequences

One file to back up.
