# A NixOS module: a service for the ground station.
{ config, lib, pkgs, ... }:

let
  cfg = config.services.orbit;
  inherit (lib) mkEnableOption mkIf mkOption types;
  port = 8080;
in
{
  options.services.orbit = {
    enable = mkEnableOption "the orbit service";

    port = mkOption {
      type = types.port;
      default = port;
      description = ''
        The port it listens on.
      '';
    };

    stations = mkOption {
      type = with types; listOf str;
      default = [ "kiruna" "wallops" ];
    };
  };

  config = mkIf cfg.enable {
    systemd.services.orbit = {
      wantedBy = [ "multi-user.target" ];
      serviceConfig.ExecStart = "${pkgs.orbit}/bin/orbit --port ${toString cfg.port}";
    };

    networking.firewall.allowedTCPPorts = [ cfg.port ];
    environment.etc."orbit.conf".text = builtins.toJSON { inherit (cfg) stations; };
    nixpkgs.overlays = [ (final: prev: { orbit = final.callPackage ./pkg.nix { }; }) ];
    assertions = [ { assertion = cfg.port > 1024 || false; message = "unprivileged"; } ];
    path = ./config/orbit.conf;
    url = https://example.org/orbit;
    count = 3 + 4.5e2 - -1;
    ok = if cfg.enable then true else null;
    nested = rec { a = 1; b = a; }.b;
    msg = "tab\t${lib.concatStringsSep "," cfg.stations}\n";
    x = cfg.stations or [ ];
  };
}
