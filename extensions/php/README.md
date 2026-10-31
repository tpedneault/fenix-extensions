# PHP

PHP in Fenix: highlighting, folding and indenting from tree-sitter-php's
grammar, Intelephense or Phpactor as the language server, the project's
own Pint or PHP CS Fixer as the formatter, PHPStan on save, Composer's
scripts and the project's tools as tasks, the project doctor's checks
with their fixes, snippets, and a Composer package in the new-project
wizard. Built on lang-kit (`crates/lang-kit`), as each language pack is.

The project's own tools -- PHPUnit, Pest, PHPStan, Pint, PHP CS Fixer --
are found in its `vendor/bin` first, then where `composer global require`
puts them (`~/.config/composer/vendor/bin`, `~/.composer/vendor/bin`,
`%APPDATA%\Composer\vendor\bin`). PHP itself is looked for on `PATH`,
Laravel Herd's folders, Homebrew's and `C:\php`.

## The language

Fenix has no PHP of its own: the pack's language file brings
tree-sitter-php 0.24.2's grammar, compiled to WebAssembly (where it came
from, and its SHA-256, in `grammars/README.md`), and its queries. `.php`,
`.phtml`, `.php4` to `.php8`, `.phps` and Laravel's `artisan` are PHP. The
HTML around the PHP is read but not coloured: Fenix doesn't highlight one
language inside another. `gcc` comments with `//`.

## The language server

`php.server` picks one; either is used in place of the other when it's
the one installed.

| | Server | Installed by its fix row |
|---|---|---|
| `intelephense` (the default) | `intelephense --stdio` | `npm install -g intelephense` |
| `phpactor` | `phpactor language-server` on `PATH`, else the pack's: `php ${tools}/phpactor/phpactor.phar language-server` | Phpactor 2026.10.07.0's PHAR, checked against its SHA-256, into the pack's tools folder |

Phpactor's PHAR needs PHP 8.2 or later.

## Formatting

`SPC c F` -- and saving, with `editor.format_on_save` -- runs the
project's own formatter with its config: Pint where the project uses it
(`pint.json`, `laravel/pint` in `composer.json`, or `vendor/bin/pint`),
else PHP CS Fixer (`.php-cs-fixer.php` or `.php-cs-fixer.dist.php`,
`friendsofphp/php-cs-fixer`). Neither formats text on its stdin, so PHP
runs it on a copy of the buffer and gives the copy back; a file it can't
parse is left as it was. A project with neither is formatted by the
language server. `php.formatter` set to `server` always asks the server.

## PHPStan

When a PHP file is saved in a project that uses PHPStan (`phpstan.neon`,
`phpstan.neon.dist`, `phpstan/phpstan` or Larastan in `composer.json`),
the project's PHPStan analyses it with the project's config (`analyse
--error-format=json`) and its problems are shown with the server's,
each with its identifier (`argument.type`) and its tip. `php.phpstan`
turns it off.

## Tasks

`SPC p t`: each of `composer.json`'s scripts (`composer run-script`), the
everyday ones first (`test`, `lint`, `analyse`, `format`); Composer runs
its `pre-` and `post-` event scripts itself. Where no script does it:
`test` (the project's Pest, else PHPUnit), `phpstan` (`analyse`) and
`format` (Pint, or `php-cs-fixer fix`). Laravel's `serve` (`php artisan
serve`). And Composer's `install`, `update`, `outdated` (`--direct`) and
`audit`. The project's `.env` goes to each; a task of the same name in
`.fenix/tools.json` wins. Saving `composer.json`, `composer.lock` or a
tool's config, or a task that changes them, makes them again.

## The project doctor

| Row | | Fix |
|---|---|---|
| PHP | `php --version`, held to `composer.json`'s `require.php` (Composer's `^`, `~`, `\|`, `8.3.*`), else 8.2 at least | |
| Composer | found, and needed when there's a `composer.json` | |
| vendor | each package in `composer.lock` installed at its version (`vendor/composer/installed.json`); no `vendor`, or no lockfile, is a warning | `composer install` |
| Intelephense or Phpactor | the server `php.server` picks | `npm install -g intelephense`; Phpactor's release |
| Pint, PHP CS Fixer, PHPStan | where the project uses them, found in `vendor/bin` | `composer install` |

A fix that installs something is checked again, and what it installed
is used at once.

## Snippets

Files: `php` (`<?php`, strict types, a namespace). Types: `class`,
`rclass` (a readonly class), `ctor` (constructor promotion), `enum` (a
backed enum), `iface`, `trait`. Functions: `fn`, `met`, `arrow`. Control:
`foreach`, `forek` (with the key), `match`, `try`. Tests: `test` (a
PHPUnit test case), `tm` (a test method), `dp` (with a data provider),
`it` (Pest). And `dd`.

## Project templates

- **PHP · Composer package**: PSR-4 autoloading from `src/` under the
  namespace you give, strict types, PHPUnit with a data provider, and
  `composer test`; made with `composer install`.

## Settings

| Setting | Default | |
|---|---|---|
| `php.server` | `intelephense` | `intelephense`, `phpactor` or `off` |
| `php.formatter` | `project` | `project` (Pint or PHP CS Fixer, as the project has) or `server` |
| `php.phpstan` | on | PHPStan on a saved file |

## Keys

After `SPC m` in a PHP project or file: `d` checks the project again, `i`
runs `composer install`.

## Not yet

- **Debugging**: Xdebug needs its DAP adapter, which is Node and VS
  Code's; it comes in a later version.
- **Blade templates** (`*.blade.php`): read as PHP, without Blade's own
  directives.
