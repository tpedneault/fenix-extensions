# Changelog

## 1.2.0

- Fenix 1.6 and later show a project with a `.luarc.json`, `.luarc.jsonc`, `selene.toml`, `stylua.toml`, `.stylua.toml`, `.luacheckrc`, `.busted` or `conf.lua` at its top as a Lua project (`LUA`), in the project hub, on Home, in the modeline and in the doctor, and open it on `main.lua` or `init.lua` when you've had none of its files open; `kind = "lua"` in a project's `.fenix/settings.toml` says it by hand. Fenix 1.5 ignores all this and the pack works there as before: it's only data in the manifest, so it needs no newer Fenix and no newer API.

## 1.1.0

- For Fenix 1.5 (extension API 1.5): how installing lua-language-server,
  StyLua or selene ended is a notification rather than a line in the
  message line, since the download can take a while and you may be
  elsewhere -- "Installed lua-language-server 3.19.2", and what it's now
  used as; or "Couldn't install StyLua 2.6.0", saying whether its
  download, unpacking it or setting it up failed and why, with *Try
  again*. The newest for a tool takes the last one's place, and a
  failure's is taken away when you try again. Starting an install is
  still said in the message line. luacheck and busted, installed by
  `luarocks` as a task, are as they were. The README gives standard keys
  beside Vim's.

## 1.0.0

- The first release, for Fenix 1.0: lua: lua-language-server, StyLua formatting, selene or luacheck on save, busted, LuaRocks and LÖVE tasks, the project's checks with their fixes, snippets, and LÖVE game and LuaRocks module projects.
