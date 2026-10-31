# Lua

Lua in Fenix: lua-language-server, StyLua formatting, selene or luacheck
on save, tasks for scripts, busted, LuaRocks and LÖVE, the project
doctor's checks with their fixes, snippets, and LÖVE game and LuaRocks
module projects in the new-project wizard. Built on lang-kit (`crates/lang-kit`), as each language
pack is.

Fenix highlights Lua itself and knows its comment, so the pack brings no
grammar. It starts with Fenix: the server is set for every Lua file, and
the doctor and the tasks are there in a project with Lua in it (a `.lua`
file, a rockspec or a `.luarc.json`).

## The server

[lua-language-server](https://luals.github.io) (LuaLS), in place of
Fenix's own, when it's found. Its fix row installs LuaLS 3.19.2's release
for this machine (Linux and macOS on x86_64 and arm64, Windows on
x86_64), checked against the SHA-256 the pack knows and unpacked into the
pack's tools folder, where only the pack looks. One you installed
yourself is used instead: the pack looks on `PATH` and in Neovim's Mason
folder, Homebrew's, LuaRocks' and Scoop's.

It's given:

- `workspace.checkThirdParty = "ApplyInMemory"`: LuaLS's own definitions
  for LÖVE, busted, luassert, OpenResty and the rest are used when your
  code uses them, without the question Fenix can't answer.
- In a LÖVE game or a Neovim config or plugin, `runtime.version =
  "LuaJIT"`; in Neovim's, `vim` as a known global.

A `.luarc.json` wins over all of it: the place for the Lua version, more
globals, or Neovim's runtime as a library.

## Formatting

`SPC c F`, and saving with `editor.format_on_save`: StyLua in a project
with a `stylua.toml` or `.stylua.toml` (found from the file's folder up),
else the server's own formatting, which reads `.editorconfig`. The
doctor's fix row installs StyLua 2.6.0. `lua.formatter` is `auto`,
`stylua` (StyLua everywhere it's installed) or `server`.

## Linting

The server's checks as you type, and when a Lua file is saved:

- **selene** in a project with a `selene.toml`. Its fix row installs
  selene 0.31.0 (Linux and Windows on x86_64, macOS on arm64).
- **luacheck** in a project with a `.luacheckrc`. Its fix row runs
  `luarocks install --local luacheck`.

`lua.linter` is `auto`, `selene`, `luacheck` or `off`.

## Tasks

`SPC p t`, as the project has them; `.env`'s values go to each.

| Task | Runs | When |
|---|---|---|
| run | `lua main.lua` (LuaJIT without Lua) | a `main.lua` that isn't a game |
| love | `love .` | a LÖVE game: `main.lua` with `conf.lua`, or `love.` callbacks |
| test | `busted` | a `.busted` or a `spec` folder |
| luarocks make, luarocks test | `luarocks make`, `luarocks test` with the rockspec | a rockspec at the top; a pair each, by name, for several |
| lint | `selene .` or `luacheck .` | the linter in use, installed |
| format | `stylua .` | StyLua in use, installed |

## The doctor

| Check | | Fix |
|---|---|---|
| Lua (or LuaJIT) | `lua -v`; only a suggestion in a LÖVE game or a Neovim config | |
| LuaRocks | with a rockspec | |
| lua-language-server | | its release |
| StyLua | when it formats | its release |
| selene, luacheck | the linter in use | its release; `luarocks install --local luacheck` |
| busted | with specs | `luarocks install --local busted` |
| LÖVE | for a game | |

`SPC m d` checks again.

## Settings

| Setting | Default | |
|---|---|---|
| `lua.formatter` | `auto` | StyLua with its config, else the server |
| `lua.linter` | `auto` | selene with `selene.toml`, luacheck with `.luacheckrc` |

## Files

Besides `*.lua`, read as Lua: `*.rockspec`, `*.wlua` (wxLua), `*.nse`
(Nmap's scripts), `.luacheckrc`, `.busted` and `config.ld` (LDoc's).

## Snippets

| Keys | |
|---|---|
| `fn`, `lfn`, `fun` | function, local function, anonymous function |
| `mod`, `req`, `class` | module table, `require`, a class with a metatable |
| `for`, `fori`, `forp`, `while` | numeric `for`, `ipairs`, `pairs`, `while` |
| `if`, `ife` | `if`; `if`, `elseif`, `else` |
| `pcall` | `pcall`, its error given back |
| `describe`, `it` | busted |
| `love`, `conf` | LÖVE's callbacks, `love.conf` |
| `keymap`, `autocmd` | Neovim's `vim.keymap.set`, `nvim_create_autocmd` |

## Project templates

Two in the new-project wizard, under Lua:

- **LÖVE game**: `main.lua` with `load`, `update` and `draw`, `conf.lua`,
  `stylua.toml`, and a `.luarc.json` giving the server LÖVE's API.
- **LuaRocks module**: a rockspec (`luarocks make`, `luarocks test`),
  the module in `src/`, busted specs with their `.busted`, `stylua.toml`
  and `.luarc.json`.

## Keys

After `SPC m` in a Lua file or project: `d` checks the project again, `s`
installs lua-language-server, `f` StyLua.

## Not yet

- **Debugging**: there's no Lua debug adapter Fenix can start on its
  own yet.
- **Neovim's runtime**: the server isn't given `$VIMRUNTIME` as a library
  (the pack can't read your Neovim's); a `.luarc.json` can.
