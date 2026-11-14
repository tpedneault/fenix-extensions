{
  description = "Ground station tools";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, ... }@inputs:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        version = "1.${toString (self.lastModifiedDate or 0)}";
      in
      {
        packages.default = pkgs.stdenv.mkDerivation rec {
          pname = "orbit";
          inherit version;
          src = ./.;
          buildInputs = with pkgs; [ cmake ninja ];
          buildPhase = ''
            cmake -B build -G Ninja
            ninja -C build
          '';
          meta.license = pkgs.lib.licenses.mit;
        };

        devShells.default = pkgs.mkShell {
          packages = [ pkgs.nixd pkgs.nixfmt-rfc-style ];
          RUST_LOG = "info";
        };

        formatter = pkgs.nixfmt-rfc-style;

        checks.fmt = pkgs.runCommand "fmt" { } ''
          touch $out
        '';
      });
}
