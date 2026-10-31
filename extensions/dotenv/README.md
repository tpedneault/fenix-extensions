# Env files

`.env` files in Fenix: their values hidden but on the cursor's line,
every env file in the project side by side, profiles switched with a
backup kept or tasks run with them, a warning when Git would commit
one, keys checked against `.env.example`, and variable names completed
in code.

An env file is `.env`, `.env.<anything>` (`.env.local`, `.env.staging`)
or `<name>.env`. One whose name says `example`, `sample`, `template` or
`dist` is an example: it says what's needed and is meant to be
committed, so its values are shown and Git isn't asked about it.

## Values hidden

A value is drawn as `••••••` -- but as it is on the line the cursor's
on, so you can edit it. `SPC p e r` shows every value in the file, with
what `${VAR}` comes to after each value that has one; again hides them.
`dotenv.mask` off shows them always; `dotenv.mask_examples` hides an
example's too.

Fenix masks them itself from a file's first frame, by rules the
extension gives it: a value on one line, quoted or not. A
quoted value over several lines is masked once you're in the file. The
rules go by file name: the usual env files' and those in the project,
or `.env`, `.env.*` and `*.env` when examples are masked too. A file
whose values `SPC p e r` shows takes its name out of them, so another
file of that name opened then is masked once you're in it.

## Highlighting

Env files are highlighted as shell (`languages/dotenv.toml`): Fenix
ships no dotenv grammar, and of those it has, the shell's reads what an
env file holds -- `KEY=value`, `export`, `#` comments, quotes,
`${VAR}`. A value with a space that isn't quoted reads as a command
after it. It names `.env`, `*.env` and the pattern `.env.*`; a
pattern wins over a file's extension, so `dotenv_*.toml` give
code named like one (`.env.d.ts`, `.env.mjs`, `.env.json`, `.env.yml`)
its own language back. With `bash-language-server` installed it's asked about them
too, as shell; ShellCheck may then say a key is unused (SC2034).

## Checks

As you type, an env file's problems are underlined:

- a line that isn't `KEY=value` or a comment, a quote not closed, text
  after a closing quote, a key a shell can't take (`my.key`, `1X`);
- a key set twice: most loaders keep the last, dotenvy the first;
- `${VAR}` that no key above sets (it's the environment's, if anything);
- the keys the example beside it has and it lacks (`SPC p e m` adds
  them, empty, as one change you can undo), and the keys it has that
  the example hasn't;
- Git: a file it tracks (its secrets are in the history: `SPC p e u`
  stops tracking it, the file kept, then commit that), or one
  `.gitignore` doesn't ignore (`SPC p e i` adds it). Asked of
  `git ls-files` and `git check-ignore` when you first open an env
  file, after `.gitignore` is saved, and with `SPC p e c`.

The project doctor's page has the same, with the fixes.

## Profiles

`SPC p e p` copies `.env.<name>` over `.env` (the one beside the env
file you're in, else the project's), once you say yes. The old `.env`
is kept first, outside the project so Git never sees it, in the
extension's folder; `SPC p e b` puts one back (keeping what's there
first). When `.env` is open the copy is one change you can undo, saved.
The modeline says `env: staging` while `.env` is `.env.staging`'s copy.

### Tasks with a profile

Or leave `.env` as it is and run your tasks with a profile's variables:
`dotenv.tasks` lists commands (`npm run dev, cargo run`), offered in the
task picker (`SPC p t`) as `npm run dev · env .env.staging`, run in the
project with the variables of the env file `SPC p e t` picked (`.env`
until you pick one; kept for the project). The values are as they're
loaded, `${VAR}` filled in; most loaders don't override a variable
that's set, so the profile's win over what `.env` says.

A task runs one of the programs the manifest names: `npm`, `npx`,
`pnpm`, `yarn`, `bun`, `node`, `deno`, `python`, `python3`, `uv`,
`poetry`, `pipenv`, `cargo`, `go`, `make`, `just`, `docker`, `dotnet`,
`php`, `ruby`, `bundle`, `rails`; another is left out, its output page
says why. Fenix's own tasks and other extensions' run without a
profile's variables.

## The env files page

`SPC p e e`: every env file in the project, side by side -- a row a key,
a column a file, values hidden. A key the example has and a file lacks
says `missing` there.

| Key | What it does |
| --- | --- |
| `hjkl`, arrows, `g` `G` | Move |
| `r` | Show or hide the values (`${VAR}` filled in) |
| `Enter` | Open the file at the key |
| `y` | Copy the value (the key, in the key's column) |
| `m` | Add the keys its example has to the focused file |
| `p` | Switch profile |
| `i` | Add what Git would commit to `.gitignore` |
| `R` | Look for the files and ask Git again |

## Completion

In code, the project's variable names are offered, each with the files
that set it, after:

| Language | |
| --- | --- |
| JavaScript, TypeScript | `process.env.`, `process.env["`, `import.meta.env.`, `Bun.env.`, `Deno.env.get("` |
| Python | `os.environ["`, `os.environ.get("`, `os.getenv("` |
| Rust | `std::env::var("`, `env::var_os("`, `env!("`, `option_env!("`, `dotenvy::var("` |
| Go | `os.Getenv("`, `os.LookupEnv("` |
| PHP | `$_ENV['`, `$_SERVER['`, `getenv('` |
| Ruby | `ENV['`, `ENV.fetch('` |

With no quote typed yet (`os.getenv(`) the name goes in quotes. It's a
language server beside the language's own: Fenix merges their
completions. In an env file, names are offered after `$` and `${`.

## Keys

| Key | Command | What it does |
| --- | --- | --- |
| `SPC p e e` | `dotenv.page` | Every env file, side by side |
| `SPC p e r` | `dotenv.reveal` | Show or hide this file's values |
| `SPC p e p` | `dotenv.profile` | Switch profile |
| `SPC p e t` | `dotenv.task_profile` | The env file tasks run with |
| `SPC p e b` | `dotenv.restore` | Put back a backup |
| `SPC p e m` | `dotenv.add_missing` | Add the keys the example has |
| `SPC p e i` | `dotenv.gitignore` | Add what Git would commit to `.gitignore` |
| `SPC p e u` | `dotenv.untrack` | Stop tracking env files in Git |
| `SPC p e c` | `dotenv.check` | Look for env files and ask Git again |

## Settings

| Setting | What it does |
| --- | --- |
| `dotenv.mask` | Hide values but on the cursor's line |
| `dotenv.mask_examples` | Hide examples' values too |
| `dotenv.git` | Ask Git whether it would commit an env file |
| `dotenv.tasks` | Commands offered as tasks, with a profile's variables |

## What it may do

- **Where you are, and change it:** hide the values of the env file
  you're in; add keys to it, copy a profile over it, add to
  `.gitignore`.
- **The project, read and changed:** its env files, read; a profile
  copied over `.env`; `.gitignore`.
- **Run git:** `ls-files` and `check-ignore`, to ask whether it would
  commit an env file; `rm --cached` when you ask it to stop tracking
  one.
- **Run npm, cargo, python and the like:** only the commands you list
  in `dotenv.tasks`, as tasks you start.
