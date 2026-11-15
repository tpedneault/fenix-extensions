{
  description = "{{name}}";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      # nix build, and nix run for the app below.
      packages = forAllSystems (pkgs: {
        default = pkgs.writeShellApplication {
          name = "{{name_kebab}}";
          runtimeInputs = [ pkgs.coreutils ];
          text = ''
            echo "Hello from {{name}}, on $(uname -s)"
          '';
        };
      });

      apps = forAllSystems (pkgs: {
        default = {
          type = "app";
          program = "${self.packages.${pkgs.stdenv.hostPlatform.system}.default}/bin/{{name_kebab}}";
        };
      });

      # nix develop: the tools Fenix finds on PATH there (SPC p h).
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.nixd
            pkgs.statix
            pkgs.deadnix
            pkgs.{{formatter}}
          ];
        };
      });

      # nix fmt, and what Fenix formats with.
      formatter = forAllSystems (pkgs: pkgs.{{formatter}});

      # nix flake check: the package builds.
      checks = forAllSystems (pkgs: {
        build = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      });
    };
}
