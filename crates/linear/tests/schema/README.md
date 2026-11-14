# Linear's schema, as far as this extension reads it

`linear-subset.graphql` is the parts of Linear's public GraphQL schema that
the queries in `src/queries.rs` touch -- every field they select, every
argument they pass, every input type and enum their variables use -- and
nothing more. The whole schema is 1.5 MB; this is a few hundred lines.

**Written against** the schema in Linear's SDK repository,
[`linear/linear`](https://github.com/linear/linear), file
`packages/sdk/src/schema.graphql`, at commit `4a8d94a542298412c543945e7da29e4cbc2a3326`
(13 November 2026), which is `@linear/sdk` 102.0.0. It was fetched by its raw
URL (a shallow clone works too); `api.github.com` was not used.

**What holds the queries to it.** `tests/schema.rs` runs every call the crate
makes on a recorded workspace and reads each request it sent: the query as
GraphQL (every field, argument and fragment must be in the schema, every
required argument given, every variable of a type its argument accepts)
and the variables as JSON (every key of a filter or input in the schema, every
value of the type it has, every enum value one it knows). The recorded
answers are held to the queries as well: a field in a fixture that its query
didn't ask for is a fixture written wrong. It fails for any query that
isn't exercised, so a new query can't go unchecked.

**Writing it again** (a new schema version, a new field in a query):

```text
curl -O https://raw.githubusercontent.com/linear/linear/master/packages/sdk/src/schema.graphql
LINEAR_SCHEMA=$PWD/schema.graphql cargo test --test schema regenerate_the_subset -- --ignored
```

That run checks the queries and variables against the **whole** schema first
-- so a field that isn't in the subset can't get in by being left out of it --
and writes the subset from what they touched. Commit the new file, and say
here which commit it came from.

**What this can't tell.** That Linear's server behaves as its schema says
(an `issue(id:)` that takes an identifier, a rate limit's headers, which
error code a missing issue has), and that the schema it was written against is
the one a workspace is on. The fixtures in `tests/fixtures` are written from
the schema and Linear's documentation, **not recorded**: no Linear
workspace was available.
