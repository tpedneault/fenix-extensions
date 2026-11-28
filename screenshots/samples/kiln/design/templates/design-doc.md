<!--
A design document, as a Kiln export template, added by `kiln preset add
design-doc`. Edit it freely: it is this project's file now. Comments like
this one don't appear in the export. `kiln check` validates this file;
`kiln export design-doc` writes it.

It reads top-down: what the system must do, how it is built and why, how
it is checked, when it ships and what could go wrong. Requirements come
first, as one tree: with several requirement levels ([requirements] in
kiln.toml), each top-level requirement has the requirements refining it
beneath, level by level. Everything is on one page; layout = "sections"
in the export target gives each top-level heading below a page of its
own.
-->

# Introduction

<!-- kiln:summary -->

This is the design document of <!-- kiln:value name -->. It says what
it must do (Requirements), how it is built and why (Design, Decisions),
how that is checked (Verification), when it ships (Plan) and what could
go wrong (Risks); the appendices trace each requirement to its design
and tests. Every item is a file in the repository: edit those, not this
document.

<!-- kiln:toc -->

## Purpose and scope

<!-- Who reads this document, what it covers, and what it leaves to other
documents. -->

## Goals and non-goals

<!-- What the design must achieve, and what it deliberately leaves out. -->

# Requirements

<!-- kiln:requirements -->

# Design

<!-- How it is built: one item per part, interface or mechanism. -->

<!-- kiln:items type="design" -->

# Decisions

<!-- Why it is built this way: accepted decisions, and the superseded
ones that explain how it got here. -->

<!-- kiln:items type="decision" status="accepted,superseded" -->

# Verification

<!-- How each requirement is checked; the appendix shows the coverage. -->

<!-- kiln:items type="test" style="row" -->

# Plan

<!-- kiln:report roadmap -->

<!-- kiln:items type="task" style="row" -->

# Risks

<!-- kiln:report risks -->

# Appendix: Traceability

<!-- kiln:report traceability -->

# Appendix: Open findings

<!-- kiln:report findings -->
