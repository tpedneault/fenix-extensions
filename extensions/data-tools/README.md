# Data tools

JSON, YAML and TOML in Fenix: formatted, minified, keys sorted,
converted, queried, checked against a JSON Schema, and types made from
them.

Each works on the selection, else the whole buffer, and is one change
you can undo. What it works on is the file's language, or, in another
file (a code block in Markdown), what the selection looks like.

- **Format** with the project's indent: `data-tools.indent` if you set
  it, else the project's `.editorconfig`, else Fenix's indent width
  (`editor.indent_width`). A selection is indented as the line it's on.
  TOML is formatted in place, its comments kept; JSON's comments (as in
  `tsconfig.json`) and YAML's don't survive being written again, and it
  says so.
- **Minify**: JSON on one line, YAML in flow style (`{a: 1, b: [2]}`).
- **Sort keys**, in every object or only the top level's. JSON on one
  line stays on one line. In TOML a comment goes with the key it's above,
  and tables' headers follow their keys' order.
- **Convert** between JSON, YAML and TOML: into a buffer beside, or in
  place. Keys stay in their order, and numbers as they were written
  (`1.50`, a 30-digit integer). TOML has no null, JSON no `.inf`: it says
  where.
- **Query** with JSONPath (`$.servers[*].port`), or a jq filter when jq
  is installed. The result opens beside, in the same buffer each time.
- **Escape** the selection as a JSON string, **unescape** one; the
  document as a string of JSON (`"{\"a\":1}"`), and such a string back as
  its document, formatted.
- **Copy the path** to the key under the cursor: `.servers[2].port`, or
  `$.servers[2].port`, or `/servers/2/port` (`data-tools.path_style`).
- **Types** from a sample: TypeScript interfaces, Rust structs with
  serde, Python dataclasses, Go structs. Each list's objects make one
  type; a key some of them lack is optional, one that's sometimes null
  nullable.

## Checks

JSON, YAML and TOML files are checked as you type: what doesn't parse is
underlined, and what the file's JSON Schema says against it. Its schema
is the first of:

1. One you picked for the file (`SPC x V`): from SchemaStore's catalog,
   or a URL or a path typed in.
2. One `data-tools.schemas` gives for files like it (`config/*.yaml`).
3. The one it names: `"$schema"`, YAML's `# yaml-language-server:
   $schema=...`, TOML's `#:schema ...`.
4. SchemaStore's for its name -- `package.json`, `tsconfig.json`, a
   GitHub workflow, `docker-compose.yml` (`data-tools.schemastore`).

A schema's path is from the file's folder. Schemas, what they refer to
and the catalog are fetched once and kept in the extension's folder, and
fetched again after a week. `SPC x v` checks the file now and says how
it went.

## Keys

| Key | Command | What it does |
| --- | --- | --- |
| `SPC x f` | `data-tools.format` | Format |
| `SPC x m` | `data-tools.minify` | Minify |
| `SPC x s` | `data-tools.sort` | Sort keys, everywhere |
| `SPC x S` | `data-tools.sort_top` | Sort the top level's keys |
| `SPC x j` `y` `t` | `data-tools.to_json`, `to_yaml`, `to_toml` | As JSON, YAML, TOML, beside |
| `SPC x c` | `data-tools.convert` | Convert in place |
| `SPC x q` | `data-tools.query` | Query: JSONPath, or jq |
| `SPC x v` | `data-tools.validate` | Check against the schema now |
| `SPC x V` | `data-tools.schema` | Pick the schema |
| `SPC x p` | `data-tools.copy_path` | Copy the key's path |
| `SPC x e` `u` | `data-tools.escape`, `unescape` | Escape, unescape a JSON string |
| `SPC x l` `L` | `data-tools.stringify`, `parse_string` | As a string of JSON, and back |
| `SPC x g` | `data-tools.types` | Types from the sample |

In a JSON, YAML or TOML file the same keys are under `SPC m`.

## Settings

| Setting | What it does |
| --- | --- |
| `data-tools.indent` | Spaces a level, or `tab`; empty follows `.editorconfig`, then Fenix |
| `data-tools.results` | Results `beside` (a new pane) or `here` |
| `data-tools.path_style` | `jq`, `jsonpath` or `pointer` |
| `data-tools.check` | Check files as you type |
| `data-tools.schemastore` | SchemaStore's schema for a file that names none |
| `data-tools.schemas` | Schemas for files by pattern |
| `data-tools.schema_host` | One more host schemas come from |

## What it may do

- **Where you are, and change it:** read the selection or the buffer, and
  put the result back.
- **The project, read:** its `.editorconfig`, and a schema in it.
- **Fenix's settings under `editor`:** its indent width.
- **Run jq**, for a query that isn't JSONPath.
- **The network:** `www.schemastore.org`, `json.schemastore.org` and
  `raw.githubusercontent.com`, for SchemaStore's catalog and schemas,
  and the host `data-tools.schema_host` names. A schema elsewhere can't
  be fetched: it says so.
