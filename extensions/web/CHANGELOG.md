# Changelog

## 1.2.0

- For Fenix 1.5 (extension API 1.5): how installing js-debug ended is a
  notification rather than a line in the message line, since the
  download can take a while and you may be elsewhere -- "Installed
  js-debug 1.140.0", and the folder it's in; or "Couldn't install
  js-debug 1.140.0", saying whether its download, unpacking it or
  setting it up failed and why, with *Try again*. A failure's is taken
  away when you try again. Starting the install is still said in the
  message line. The other fixes (the servers, Node, the package manager,
  the project's packages) run their package manager as a task, and are
  as they were. The README gives standard keys beside Vim's. On Fenix
  1.0 to 1.4, 1.1.0 stays the one you have.
- A lesson in Fenix's tutorial (`SPC h T`, its *Web: JavaScript and
  TypeScript* section), in a project of yours with a `package.json`: the
  project doctor and a fix from it, the packages page, a task, a debug
  session stopped at a breakpoint, and the formatter and ESLint on save.

## 1.1.0

- `web.packages` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): its page opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.0

- The first release, for Fenix 1.0: javaScript and TypeScript: typescript-language-server with the CSS, HTML, Tailwind, ESLint and Biome servers beside it, JSON with its schemas from Config & Shell, debugging with js-debug (Node, each of package.json's scripts, a Vite page in Chrome, Vitest and Jest, attaching to a process), Prettier or Biome, package.json's scripts as tasks, the project's checks with their fixes, outdated and vulnerable packages for npm, pnpm, Yarn and bun, snippets, and Next.js, Express, library and static site projects.
