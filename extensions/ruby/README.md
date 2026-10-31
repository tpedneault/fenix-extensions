# Ruby

Ruby in Fenix: tree-sitter-ruby's highlighting, folding and indenting,
Ruby LSP (or Solargraph) as the language server, RuboCop -- or Standard
-- as the formatter and the linter on save with its corrections,
Bundler's, Rake's and the tests' tasks, the project doctor's checks with
their fixes, and snippets. Built on lang-kit (`crates/lang-kit`), as each
language pack is.

Fenix has no Ruby of its own, so the pack brings the grammar
(`languages/ruby.toml`, `grammars/`, `queries/ruby/`). It works for a
project Bundler manages (a Gemfile) and for a folder of scripts: a gem
the project's Gemfile.lock pins runs as the project's, through `bundle
exec`; anything else, as it's installed.

## Files

Besides `*.rb`, these are Ruby: `*.rbw`, `*.rake`, `*.gemspec`, `*.ru`
(`config.ru`), `*.jbuilder`, `*.builder`, `*.thor`, `*.podspec`, and
`Gemfile`, `gems.rb`, `Rakefile`, `Guardfile`, `Capfile`, `Vagrantfile`,
`Podfile`, `Fastfile`, `Appfile`, `Dangerfile`, `Berksfile`, `Brewfile`,
`Steepfile`, `Thorfile`, `.irbrc`, `.pryrc`, `.simplecov`. `gcc` comments
with `#`.

A `def`, a `class` or a `module`, a block, an `if`, a `case`, a loop and
a `begin` fold, and are the scopes `SPC c s` and the breadcrumbs list.
Enter after a line that opens one indents, and after an `else`, a `when`
or a `rescue`.

## The language server

Ruby LSP, `ruby-lsp`, when it's installed; else Solargraph, `solargraph
stdio`. `ruby.server` says which to prefer when both are. Ruby LSP runs as
it's installed, never through `bundle exec`: it makes a bundle of its own
with the project's gems in it (`.ruby-lsp/`), and gives RuboCop's
problems itself when the project's bundle has RuboCop. Solargraph runs as
the project's when its Gemfile.lock pins it.

With neither, the doctor's fix row installs Ruby LSP (`gem install
ruby-lsp`). The pack looks where rbenv's, asdf's and mise's shims are,
Homebrew's Ruby and the gems it installs, and RubyInstaller's, as well as
on `PATH`.

## Linting and formatting

`SPC c F` -- and saving, with `editor.format_on_save` -- runs `rubocop
--fix-layout --stdin` on the buffer: its layout corrections only, so a
variable it thinks useless isn't taken out of a method half written.
What's left that isn't layout makes it exit 1; its text is kept.
`ruby.format` off leaves formatting to the server.

On save (`ruby.lint`), `rubocop --format json --force-exclusion` runs on
the file and its problems are shown with the server's: a syntax error as
an error, `Lint/` as warnings, the rest as notes. RuboCop 1.92's
corrections are offered on `SPC c a` (one it makes only with `-A` says
"unsafe"). `auto` does this unless Ruby LSP is the server and the
project's bundle has RuboCop, whose problems Ruby LSP gives as you type;
`save` does it whatever the server; `off` never.

A project that uses Standard (its gem in Gemfile.lock, or a
`.standard.yml`) gets `standardrb` in place of `rubocop` for each.

## Tasks

`SPC p t`:

| Task | Runs |
|---|---|
| bundle install | `bundle install`, with a Gemfile |
| test | `bin/rails test` in a Rails app with `test/`; else `bundle exec rspec` with `spec/` or RSpec in the lock; else `rake test` with a Rakefile and `test/` |
| rubocop, rubocop: correct | over the project, and with `--autocorrect` (`standardrb` and `--fix` with Standard) |
| rails server | `bin/rails server` |
| rake *task* | each task `rake -T` lists (sixty at most), the project's Rake when the lock pins it |

`rake -T` is asked in the background when the pack starts and when the
Gemfile, its lock, the Rakefile or a `.rake` file is saved; `SPC m r`
asks again. The project's `.env` goes to each task. A task of the same
name in `.fenix/tools.json` wins.

## The project doctor

| Check | | Fix row |
|---|---|---|
| Ruby | held to `.ruby-version`, else `.tool-versions`, else the Gemfile's `ruby` (`~> 3.3` is `>=3.3,<4`); 3.0 at least when none says | -- your version manager's |
| Bundler | with a Gemfile | `gem install bundler` |
| Gems | with Gemfile.lock, `bundle check --dry-run`: which of its gems aren't installed | `bundle install` (`SPC m b`) |
| Language server | Ruby LSP or Solargraph, found | `gem install ruby-lsp` (or `solargraph`) |
| RuboCop, or Standard | the project's (`bundle exec rubocop --version`), else the one installed | `gem install rubocop` (or `standard`); the project's comes with `bundle install` |

A fix runs as a task; what it installed is checked again and used at
once. Saving the Gemfile, its lock, `.ruby-version` or the Rakefile
checks again.

## Snippets

| Key | |
|---|---|
| `frozen` | `# frozen_string_literal: true` |
| `class`, `module`, `def`, `defs`, `attr` | a class with `initialize`, a module, a method, a class method, `attr_reader` |
| `each`, `do` | a block |
| `struct`, `data` | `Struct.new`, `Data.define` |
| `case`, `casein` | `case`/`when`, `case`/`in` pattern matching |
| `begin` | `begin`/`rescue`/`ensure` |
| `main` | `if $PROGRAM_NAME == __FILE__` |
| `describe`, `context`, `it`, `let` | RSpec |
| `test` | a Minitest test case |

## Settings

| Setting | Default | |
|---|---|---|
| `ruby.server` | `ruby-lsp` | `solargraph` to prefer Solargraph |
| `ruby.lint` | `auto` | RuboCop on save: `auto`, `save`, `off` |
| `ruby.format` | on | RuboCop's layout corrections as the formatter |

## Keys

After `SPC m` in a Ruby file: `d` checks the project again, `b` installs
the gems (`bundle install`), `r` reads the project again and asks Rake for
its tasks.

## Not yet

- **Ruby installed for you**: the doctor says which Ruby the project
  wants; rbenv, asdf, mise or RubyInstaller installs it.
- **Gems from releases**: gems aren't single programs with a release to
  download and check, so the fix rows run `gem install`, into the Ruby
  that's found. A system Ruby's (`/usr/bin/ruby`) may need `sudo`, which
  a task can't give: a version manager's Ruby doesn't.
- **ERB**: `*.erb` is HTML with Ruby in it, which Fenix doesn't highlight
  one inside the other.
- **Debugging**: rdbg's adapter, in a later version.
