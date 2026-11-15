# {{name}}

A NixOS module in a flake, started from Fenix's *Nix · NixOS module*
template.

| File | What it holds |
|---|---|
| `module.nix` | the options (`services.{{name_snake}}.enable`, `.port`, `.package`) and the systemd service they turn on |
| `flake.nix` | `nixosModules.default`, and `nixosConfigurations.example`, which exists to evaluate the module |

**Flakes only see what Git tracks.** Run `git init` and `git add .` first.

In Fenix, `SPC m b` lists the flake's outputs from `nix flake show` --
pick `nixosConfigurations.example` to build the example system (it writes
`./result`, and changes nothing on the machine). `nixos build` and `nixos
dry-build` in `SPC p t` run `nixos-rebuild` on the host's own configuration
when you have one. There's no `switch` task: switching changes the
running system, so it's a command you type.

To use the module, import it in your configuration's flake:

```nix
inputs.{{name_kebab}}.url = "path:/path/to/{{name_kebab}}";
# ...
modules = [ inputs.{{name_kebab}}.nixosModules.default ];
```
