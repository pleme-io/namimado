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

    enableMcpBin = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Install a `namimado-mcp` shim on PATH that runs `namimado mcp` —
        useful for AI agents (Claude Code, Cursor, OpenCode) via
        blackmatter-anvil.

        Register with anvil:

          blackmatter.components.anvil.mcp.servers.namimado = {
            command = "''${pkgs.namimado}/bin/namimado-mcp";
            args = [ ];
            enableFor = [ "claude-code" "cursor" ];
          };
      '';
    };

    enableHttpService = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Install a launchd/systemd user service that runs
        `namimado serve --addr 127.0.0.1:7860`. The inspector UI
        becomes available at http://127.0.0.1:7860/ui whenever the
        user is logged in.
      '';
    };

    httpAddr = mkOption {
      type = types.str;
      default = "127.0.0.1:7860";
      description = "Listen address for the HTTP server (when enableHttpService is true).";
    };
  };

  config = mkIf cfg.enable (mkMerge [
    {
      home.packages = [ cfg.package ];
    }

    (mkIf cfg.enableMcpBin {
      # Shim that exposes `namimado mcp` under a stable name for
      # MCP-agent config files. `namimado` itself runs the GUI;
      # `namimado-mcp` is the stdio MCP entry point.
      home.file.".local/bin/namimado-mcp" = {
        executable = true;
        text = ''
          #!${pkgs.bash}/bin/bash
          exec ${cfg.package}/bin/namimado mcp "$@"
        '';
      };
    })

    (mkIf cfg.enableHttpService {
      # User-scope service. Darwin uses launchd; Linux uses systemd.
      # hmHelpers.mkPlatformService abstracts both.
      launchd.agents.namimado-serve = mkIf pkgs.stdenv.isDarwin {
        enable = true;
        config = {
          ProgramArguments = [
            "${cfg.package}/bin/namimado"
            "serve"
            "--addr"
            cfg.httpAddr
          ];
          KeepAlive = true;
          RunAtLoad = true;
          StandardErrorPath = "${config.home.homeDirectory}/Library/Logs/namimado.err.log";
          StandardOutPath = "${config.home.homeDirectory}/Library/Logs/namimado.out.log";
        };
      };
      systemd.user.services.namimado-serve = mkIf pkgs.stdenv.isLinux {
        Unit = {
          Description = "namimado — Lisp-substrate browser HTTP + MCP server";
          After = [ "graphical-session.target" ];
        };
        Service = {
          ExecStart = "${cfg.package}/bin/namimado serve --addr ${cfg.httpAddr}";
          Restart = "on-failure";
        };
        Install.WantedBy = [ "default.target" ];
      };
    })
  ]);
}
