{
  description = "{{name}}: a NixOS module";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    {
      # What other flakes import: `inputs.{{name_kebab}}.nixosModules.default`.
      nixosModules.default = ./module.nix;

      # A configuration that only exists to evaluate the module: nothing here
      # is a machine. `nix build .#nixosConfigurations.example.config.system.build.toplevel`
      # (SPC m b) builds it; it doesn't change the one you're running.
      nixosConfigurations.example = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          self.nixosModules.default
          {
            services.{{name_snake}}.enable = true;
            boot.loader.grub.enable = false;
            fileSystems."/" = {
              device = "/dev/null";
              fsType = "ext4";
            };
            system.stateVersion = "24.11";
          }
        ];
      };
    };
}
