# Web: JavaScript and TypeScript

JavaScript and TypeScript in Fenix: typescript-language-server with the
CSS, HTML, Tailwind CSS, ESLint and Biome servers beside it,
debugging with js-debug, Prettier or Biome as the formatter,
`package.json`'s scripts as tasks, the project doctor's checks with
their fixes, outdated and vulnerable packages, snippets, and Next.js,
Express, library and static site projects in the new-project wizard.
Built on lang-kit (`crates/lang-kit`), as each language pack is.
It needs Fenix 1.5 or later (extension API 1.5); on an older Fenix, Web
1.1.0 is the one you get. It depends on Config & Shell (`fenix.config`),
installed with it, which serves JSON: `package.json`, `tsconfig.json`
and the rest get SchemaStore's schemas.

Its package manager is the one the lockfile says: `package-lock.json`
npm, `pnpm-lock.yaml` pnpm, `yarn.lock` Yarn (2 and later, berry, when
there's a `.yarnrc.yml` or the lockfile says so), `bun.lock` bun; with
none, corepack's `packageManager`, else npm.

## The language servers

Fenix's own typescript-language-server stays the one for JavaScript,
TypeScript and TSX; when it's found somewhere Fenix's `PATH` may not
have (npm's global prefix, Volta's, pnpm's), it's started from there.
Beside it (`language-server-add`), their completions,
problems and code actions merged with its:

| | |
|---|---|
| `vscode-css-language-server`, `-html-` | CSS and HTML, in any project (vscode-langservers-extracted) |
| `tailwindcss-language-server` | HTML, CSS and scripts, when the project uses Tailwind (`tailwindcss`, `@tailwindcss/vite`, a `tailwind.config.*`) |
| `vscode-eslint-language-server` | scripts, when the project uses ESLint (`web.eslint` = `server`, the default) |
| `biome lsp-proxy` | scripts, CSS and JSON, when the project uses Biome: its lints as you type |

ESLint's server gives its problems when Fenix asks for them (pull
diagnostics), and is given the settings it asks for
(`workspace/configuration`): VS Code's, with flat config when the
project has an `eslint.config.*` and the project's package manager. Its
fixes, *Disable no-var for this line* and *Fix all auto-fixable
problems* are on `SPC c a`. A project's `.fenix/lsp/eslint.json` is
read under them. Tailwind's server gets its answers too, and starts.

## Linting and formatting

With `web.eslint` = `save`, or while ESLint's server isn't installed,
the project's ESLint (`node node_modules/eslint/bin/eslint.js --format
json <file>`) runs on a script when it's saved, its problems shown with
the server's: its errors as errors, its warnings as warnings, a file it
ignores left alone. `web.eslint` = `off` is neither.

`SPC c F` -- and saving, with `editor.format_on_save` -- runs Biome
(`biome format --stdin-file-path=<file>`) in a project with a
`biome.json` or `@biomejs/biome`, else Prettier (`prettier
--stdin-filepath <file>`) in one that uses it (`prettier` a dependency,
a `.prettierrc`, `package.json`'s `prettier`): the project's own from
`node_modules`, run by `node`, else the one installed. A project that
uses neither isn't formatted by it.

## Debugging

The adapter is js-debug's DAP server, installed into Fenix's tools
folder from its release (`js-debug-dap-v1.140.0.tar.gz`, the same on
every machine, its SHA-256 checked) by the doctor's fix row or *Web:
install js-debug* (started in the message line; how it ended is a
notification: "Installed js-debug 1.140.0", or "Couldn't install
js-debug 1.140.0" with which step failed and why, and *Try again*), and
run as `node dapDebugServer.js ${port} 127.0.0.1`:
Fenix starts it on a free port and connects (`debug-adapter-connect`,
API).

`SPC u u`, in a project with no launch configuration of its own, offers:

| | |
|---|---|
| Node: current file | the file you're in (TypeScript by Node 22.18 and later, which strips its types) |
| Node: attach (localhost:9229) | to `node --inspect app.js` |
| Node: attach to a process | to a Node running here, picked as it starts (`${pickProcess}`) |
| Vite: dev server in Chrome (localhost:5173) | Chrome on the dev server's page (`pwa-chrome`); start the server first |
| Vitest: current file, Vitest: all tests | `vitest run`, the file or all |
| Jest: current file, Jest: all tests | `jest --runInBand` |

Beside these, each of `package.json`'s scripts is a launch of its own,
run by the project's manager with child processes debugged too (*Node:
pnpm run dev*, *Node: pnpm run test*).

js-debug debugs each program in a session of its own, which it asks the
editor to start (DAP's reverse request `startDebugging`, a second
connection to it with the target's id); Fenix opens it, and
shows it under the session it came from.

## Tasks

`SPC p t`: each of `package.json`'s scripts, run with the project's
manager (`pnpm run dev`) -- dev, start, build, test, lint, format,
typecheck and preview first -- and `install`. A `pre` or `post` script
runs with the one it's named for, so it isn't listed. A task of the
same name in `.fenix/tools.json` wins. Saving `package.json` makes them
again.

*Web: start the dev server* runs the dev (or start) script in the
background, beside the task pane, so the browser launch has
its server; run again, it shows its output; *Web: stop the dev server*
stops it.

## Tests, for Fenix 1.6

Fenix 1.6 brings a test explorer, and this pack carries the test
provider it will call for Vitest and Jest: built and tested here,
offered from Fenix 1.6. Fenix asks for it only once this pack is
released for extension API 1.6, after Fenix 1.6.0; until then nothing
changes for you, on Fenix 1.5 as on a nightly. What it will do, as
tested on what Vitest 5.1 and Jest 30.5 printed:

- **Find the runner** for each package folder, the project's and each
  workspace's (`workspaces`, `pnpm-workspace.yaml`): Vitest or Jest in
  its `package.json`, or their config files; where the root's config
  runs its packages as `projects`, the root's. Run by Node from the
  `node_modules` Node finds it in (a workspace's hoisted one too), as
  the launch files run it, never by `npx` or a `.bin` shim; a Yarn
  Plug'n'Play project by `yarn vitest`, `yarn jest`. A runner that isn't
  installed says so, and a project without one has no tests page.
- **Find the tests in a file as you type**, nothing run: `describe`,
  `it` and `test` with a string for a title, at their lines, in their
  describes; `.skip`, `xit` and a skipped describe's tests as skipped,
  `.todo` as "todo"; `.each` and a title with `${..}` as templates whose
  cases appear once listed or run. A title that isn't a string, and a
  `describe.each`'s tests, come from the listing: a run whose `-t`
  matches nothing, through the pack's reporter, which loads every test
  file but runs none.
- **Run** one, a describe, a file, a package, everything or the failed
  ones, a step per package: a file by its path, tests by their names as
  `-t`'s regex (escaped; joined by ` > ` for Vitest, spaces for Jest).
  Two tests of one name are kept apart (the second is `name (2)`), but
  run together: `-t` can't tell them apart.
- **Read** each result as it ends: a failure at its stack's first frame
  in the project, with its message, its diff and (Vitest) what it
  logged; a file that doesn't load as "didn't load", never as failing
  tests; a worker that dies errors its file's tests, and the others'
  results stand.
- **Debug one test**: at once, js-debug runs the runner's script with
  its file and `-t` its name (Jest `--runInBand`), as *Vitest: current
  file* and *Jest: current file* do.

What it can't: tests a config's own `include` or `testMatch` adds
outside `*.test.*`, `*.spec.*` (and Jest's `__tests__`) aren't found as
you type, only when listed or run; Jest gives no test's output of its
own; Deno's and bun's runners aren't served.

## The project doctor

- **Node**: held to `.nvmrc` or `.node-version` (`20.11.0` is that or a
  later 20; `lts/iron` is 20), else `engines.node`, else 18 at least;
  `web.node` names the one to use. Fix: `fnm install`, else `volta
  install node@<version>`.
- **Package manager**: the lockfile's, at corepack's `packageManager`
  major. Fix: `corepack enable <manager>` when `packageManager` names
  it, else `npm install -g <manager>`.
- **node_modules**: in step with the lockfile, by what each manager
  leaves there -- npm's `node_modules/.package-lock.json`, pnpm's
  `node_modules/.pnpm/lock.yaml`, Yarn 1's `.yarn-integrity`; for berry
  and bun, the lockfile not changed since the last install. Fix: the
  manager's `install`.
- **ESLint**, **Prettier** or **Biome**: the project's, in
  `node_modules`. Fix: the install.
- **TypeScript server**, **CSS and HTML servers**, **Tailwind CSS
  server** (in a Tailwind project), **ESLint server** (with `web.eslint`
  = `server`): each with its `npm install -g`.
- **Debugger**: js-debug, with its install.

typescript-language-server runs TypeScript's `tsserver`: the project's
own, else the one installed beside it. TypeScript 7, the native
compiler, has none, and the server then never gets past "starting". In
a project without its own TypeScript, install 5 beside the server: `npm
install -g typescript-language-server typescript@5`.

A fix that installs something is checked again, and what it installed
is used at once. The checks are made in a project with a `package.json`
only.

## Packages

`SPC m p` (or *Web: packages*): lang-kit's page -- the vulnerable first,
then the outdated, the project's own marked with their group (`dev`,
`optional`, `peer`).

| | npm | pnpm | Yarn 1 | berry | bun |
|---|---|---|---|---|---|
| list | `ls --all --json` | `list --json --depth Infinity` | `list --json` | `info --all --recursive --json` | `pm ls --all` |
| outdated | `outdated --json` | `outdated --format json` | `outdated --json` | -- | `outdated` |
| vulnerable | `audit --json` | `audit --json` | `audit --json` | `npm audit --all --recursive --json` | `audit --json` |
| `u` | `install name@latest` (`--save-dev`) | `add name@latest` (`--dev`) | `add name@latest` | `up name@latest` | `add name@latest` |
| `U` | `update` | `update` | `upgrade` | `up -R` the outdated | `update` |
| `a` | `install name` | `add name` | `add name` | `add name` | `add name` |
| `d` | `uninstall name` | `remove name` | `remove name` | `remove name` | `remove name` |

One the project doesn't ask for is upgraded within the ranges that bring
it (`npm update name`, `yarn up -R name`). What's typed after `a` goes
as typed (`vitest -D`, `zod@3`), and `npm search` finds it. Berry has no
`outdated`, so its page shows what's installed and vulnerable only.

## Snippets

Scripts: `imp`, `impd`, `impa` (imports), `exf`, `exd`, `af`, `afn`,
`aaf` (functions, async), `tryc` (await in try), `pall` (`Promise.all`),
`fetchj`, `forof`, `cl`; tests: `desc`, `it`, `ait`, `bfe`, `afe`, `vt`
(a Vitest file), `vmock`, `jmock`. React, in `.jsx` and `.tsx`: `rfc`,
`rfce`, `us`, `ue`, `um`, `uc`, `ur`, `hook`, `ctx`, and in `.tsx`
`rfct` (its props typed). TypeScript: `int`, `type`, `union`,
`asconst`, `guard`, `gfn`, `atfn`.

## Project templates

Fenix's wizard already has a Vite app (React, Vue, Svelte, Preact, Solid,
Lit or vanilla, in TypeScript); this adds:

- **Next.js app**: create-next-app's, TypeScript, the App Router and
  ESLint, Tailwind and `src/` if you like, with the manager you pick.
- **Express API**: in TypeScript, the app apart from its server so the
  tests call it on a port of their own, run by tsx as you edit, built
  by tsc.
- **TypeScript library**: ES modules with their types, built into
  `dist/`, tested by Vitest, Prettier or Biome.
- **Static site**: a page, its styles and a module, and a small Node
  server for development; nothing to install.

## Files

`*.jsonc`, `*.webmanifest`, `*.code-workspace`, `.babelrc`, `.eslintrc`,
`.prettierrc`, `.swcrc` and the like are read as JSON; `*.pcss` and
`*.postcss` as CSS.

## The lesson

`SPC h T`, Fenix's tutorial, has a lesson in its *Web: JavaScript and
TypeScript* section, in a project of yours with a `package.json`: the
project doctor and a fix from it, the packages page, a task, a debug
session stopped at a breakpoint, and the formatter and ESLint on save.
Each step is checked by what's on screen -- the page or the picker in
front -- and a step whose tool isn't installed yet is skipped with `s`
on the tutorial page; the doctor says how to install it.

## Keys

After `SPC m` in a project with a `package.json` (`C-k m` with standard
keys): `p` packages, `d` check again, `i` install, `s` start the dev
server, `S` stop it.
