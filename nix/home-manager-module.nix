self: {
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.services.rlru;
in {
  options.services.rlru = {
    enable = lib.mkEnableOption "rlru, a Rocket League replay uploader";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.rlru-dioxus-desktop;
      defaultText = lib.literalExpression "inputs.rlru.packages.\${pkgs.stdenv.hostPlatform.system}.rlru-dioxus-desktop";
      description = "The rlru desktop package to run.";
    };

    environment = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = {
        RUST_LOG = "info";
      };
      example = lib.literalExpression ''{ RUST_LOG = "info"; }'';
      description = "Environment variables to set for the rlru user service.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [cfg.package];

    systemd.user.services.rlru = {
      Unit = {
        Description = "rlru Rocket League replay uploader";
        Requires = ["graphical-session.target"];
        PartOf = ["graphical-session.target"];
        After = ["graphical-session.target"];
      };

      Service = {
        ExecStart = lib.getExe cfg.package;
        Environment = lib.mapAttrsToList (name: value: "${name}=${value}") cfg.environment;
        Restart = "on-failure";
        RestartSec = "5s";
      };

      Install = {
        WantedBy = ["graphical-session.target"];
      };
    };
  };
}
