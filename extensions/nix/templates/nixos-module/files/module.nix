# The module: what {{name_snake}} adds to a NixOS configuration. Turn it on
# with `services.{{name_snake}}.enable = true;`.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.{{name_snake}};
in
{
  options.services.{{name_snake}} = {
    enable = lib.mkEnableOption "{{name}}";

    port = lib.mkOption {
      type = lib.types.port;
      default = 8080;
      description = ''
        The port {{name}} listens on.
      '';
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.hello;
      description = ''
        The package whose main program the service runs.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.{{name_snake}} = {
      description = "{{name}}";
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package} --port ${toString cfg.port}";
        DynamicUser = true;
        Restart = "on-failure";
      };
    };

    networking.firewall.allowedTCPPorts = [ cfg.port ];
  };
}
