# Changelog

## 1.3.0

- For Fenix 1.6 (extension API 1.6): your Vitest and Jest tests on
  Fenix's tests page (`SPC T T`), in the gutter and after each test's
  line, run from any of them and debugged with `SPC T d`. Each package
  folder's runner is found from its `package.json` or config, a
  workspace's packages each a step of their own, run by Node from
  `node_modules` (a Yarn Plug'n'Play project by `yarn vitest`, `yarn
  jest`). A test is found as you type (`describe`, `it`, `test`,
  `.skip` and `.todo`, `.each` and `${..}` titles as templates whose
  cases come when listed or run); what the scan can't name comes from
  a listing that loads the test files and runs none. Runs are by file
  and by name; a failure is shown at its stack's first frame in the
  project with its message and diff; a file that doesn't load says so
  and fails no test; a worker that dies errors its own file's tests.
  Debugging a test runs its runner under js-debug with its file and
  name. On Fenix 1.5, 1.2.0 stays the one you have.

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
