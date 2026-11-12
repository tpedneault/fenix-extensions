<!-- See CONTRIBUTING.md. Delete what doesn't apply. -->

## What it changes

<!-- The extension(s), and what changes for the person using it. -->

**Way in:** a folder under `extensions/` / an `external.toml` entry

## Checklist

- [ ] `cargo xtask validate` passes
- [ ] Its tests pass (`cargo test --locked --manifest-path extensions/<name>/Cargo.toml`), and new code has tests
- [ ] `cargo xtask pack extensions/<name> dist` packs it
- [ ] README updated; `version` bumped; a CHANGELOG entry for it
- [ ] A screenshot in `screenshots/`, in the house style
- [ ] Permissions: each one new or wider is below, with its reason, and in the README's **What it may do**
- [ ] Tried in Fenix: version `...`, on `...` (system)

**Permissions asked for, and why:**

<!-- One line each: `network = ["api.example.com"]` -- its API. "None" when unchanged. -->

## For an `external.toml` entry

- [ ] Publisher: `...` -- claimed by this pull request / already mine in `publishers.toml`
- [ ] Release URL: `https://github.com/<owner>/<repo>/releases/download/...`, an asset of a repository the publisher's owner owns
- [ ] `sha256` is what `cargo xtask pack` printed for that file
