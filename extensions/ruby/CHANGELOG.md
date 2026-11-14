# Changelog

## 1.2.0

- Fenix 1.6 and later show a project with a `Gemfile`, `gems.rb`, `.ruby-version` or `config.ru` at its top (a `Rakefile` alone isn't enough: plenty of projects have one) as a Ruby project (`RB`), in the project hub, on Home, in the modeline and in the doctor, and open it on `config.ru` when you've had none of its files open; `kind = "ruby"` in a project's `.fenix/settings.toml` says it by hand. Fenix's own kinds come first: a project Fenix already takes for Rust, Python, Go, JavaScript, C/C++, Tcl or Arduino (a Rails app with a `package.json` stays `JS`) stays that one until `kind = "ruby"` says otherwise, so this kind names projects Fenix would otherwise call plain folders. Your home folder is never a project, whatever dotfiles it holds (`.ruby-version` among them). Fenix 1.5 ignores all this and the pack works there as before: it's only data in the manifest, so it needs no newer Fenix and no newer API.

## 1.1.0

- For Fenix 1.5 (extension API 1.5): built against API 1.5, as lang-kit,
  the kit it's on, now is. Nothing you see changed: the fixes that
  install Ruby's tools run `gem install` and `bundle install` as tasks,
  in Fenix's task pane, not as a download of the pack's own, so there is
  no install notification for it, and the grammar, the language server,
  RuboCop and the tasks work as they did. The README gives standard keys
  beside Vim's. On Fenix 1.0 to 1.4, 1.0.0 stays the one you have.

## 1.0.0

- The first release, for Fenix 1.0: ruby: tree-sitter-ruby's highlighting, folding and indenting, Ruby LSP or Solargraph, RuboCop and Standard formatting and linting with their corrections, Bundler's, Rake's, RSpec's and Rails' tasks, the project's checks with their fixes, and snippets.
