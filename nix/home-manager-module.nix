self: {
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.services.rlru;
  configArgs = lib.optionals (cfg.configFile != null) [
    "--config"
    (toString cfg.configFile)
  ];
in {
  options.services.rlru = {
    enable = lib.mkEnableOption "rlru, a Rocket League replay uploader";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.rlru;
      defaultText = lib.literalExpression "inputs.rlru.packages.\${pkgs.stdenv.hostPlatform.system}.rlru";
      description = "The rlru package to run.";
    };

    configFile = lib.mkOption {
      type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
      default = null;
      description = "Optional rlru TOML config file path.";
    };

    environment = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = {};
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
        ExecStart = lib.escapeShellArgs (
          [
            (lib.getExe cfg.package)
          ]
          ++ configArgs
          ++ [
            "sync"
            "daemon"
          ]
        );
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
