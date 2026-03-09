{ hmHelpers }:
{
  lib,
  config,
  pkgs,
  ...
}:
with lib;
let
  cfg = config.programs.namimado;
in
{
  options.programs.namimado = {
    enable = mkOption {
      type = types.bool;
      default = false;
      description = "Enable the namimado desktop web browser.";
    };

    package = mkOption {
      type = types.package;
      default = pkgs.namimado;
      description = "The namimado package to install.";
    };
  };

  config = mkIf cfg.enable {
    home.packages = [ cfg.package ];
  };
}
