{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.services.nc-bookmark-sync;

  toEnv = xs: mapAttrsToList (x: y: "${x}=${y}") xs;

  tomlFormat = pkgs.formats.toml { };

  pairCfg = { config, name, ... }: {
    options = {
      name = mkOption {
        type = types.str;
        default = name;
        description = "The name of the pair";
      };

      a = mkOption {
        type = types.submodule storageCfg;
        description = "Local storage";
      };

      b = mkOption {
        type = types.submodule storageCfg;
        description = "Remote storage";
      };

      conflict_resolution = mkOption {
        type = types.enum [ "error" "a wins" "b wins" ];
        default = "error";
        description = "How to handle conflicts";
      };
    };
  };

  storageCfg = { config, ... }: {
    options = {
      type = mkOption {
        type = types.enum [ "nextcloud" "file" ];
        description = "Storage type";
      };

      url = mkOption {
        type = types.nullOr types.str;
        default = null;
        example =
          "https://cloud.example.com/index.php/apps/bookmarks/public/rest/v2";
        description =
          "The api url of the Nextcloud bookmarks app. Only used for Nextcloud storages.";
      };

      path = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "/home/john/.local/share/bookmarks/cloud.example.com";
        description =
          "The path to the bookmarks file. Only used for file storages.";
      };

      username = mkOption {
        type = types.nullOr (types.submodule commandOpts);
        default = null;
        description = "Nextcloud username";
      };

      password = mkOption {
        type = types.nullOr (types.submodule commandOpts);
        default = null;
        description = "Nextcloud password";
      };
    };
  };

  commandOpts = { config, ... }: {
    options = {
      fetch = mkOption {
        type = types.listOf types.str;
        example = [ "command" "echo" "john" ];
        description =
          "This command is run to fetch the value. The first entry of the list should always be 'command'.";
      };
    };
  };

  mkAttrSet = pair: {
    pair."${pair.name}" = {
      a = "${pair.name}_local";
      b = "${pair.name}_remote";
      conflict_resolution = pair.conflict_resolution;
    };

    storage = {
      "${pair.name}_local" = filterAttrs (n: v: v != null) pair.a;

      "${pair.name}_remote" = filterAttrs (n: v: v != null) pair.b;
    };
  };

  toml = mkMerge ([{ general.status_path = cfg.status_path; }]
    ++ (mapAttrsToList (_: mkAttrSet) cfg.pairs) ++ [ cfg.extraConfig ]);

in {
  options.services.nc-bookmark-sync = {
    enable = mkEnableOption "Nextcloud Bookmark sync";

    status_path = mkOption {
      type = types.path;
      default = "${config.xdg.dataHome}/nc-bookmark-sync/status/";
      description = "Status path";
    };

    pairs = mkOption {
      type = types.attrsOf (types.submodule pairCfg);
      default = { };
      description = "Pair configuration";
    };

    extraConfig = mkOption {
      type = tomlFormat.type;
      default = { };
      description = "Extra config to append to the config.toml file.";
    };

    package = mkOption {
      type = types.package;
      default = pkgs.nc-bookmark-sync;
      defaultText = literalExample "pkgs.nc-bookmark-sync";
      description = ''
        The <literal>nc-bookmark-sync</literal> package to use.
      '';
    };

    frequency = mkOption {
      type = types.str;
      default = "hourly";
      description = ''
        How often to generate new tasks.
        default upstream.
        </para><para>
        This value is passed to the systemd timer configuration as the
        <literal>onCalendar</literal> option.
        See
        <citerefentry>
        <refentrytitle>systemd.time</refentrytitle>
        <manvolnum>7</manvolnum>
        </citerefentry>
        for more information about the format.
      '';
    };

    config = mkOption {
      internal = true;
      type = tomlFormat.type;
      default = { };
      description = "Extra config to append to the config.toml file.";
    };
  };

  config = mkIf cfg.enable {

    services.nc-bookmark-sync.config = toml;

    systemd.user.services.nc-bookmark-sync = {
      Unit = { Description = "Nextcloud Bookmark sync"; };
      Service = {
        CPUSchedulingPolicy = "idle";
        IOSchedulingClass = "idle";
        Type = "oneshot";
        Environment = toEnv {
          PATH =
            "${pkgs.gnupg}/bin:${config.programs.password-store.package}/bin:${pkgs.coreutils}/bin";
          PASSWORD_STORE_DIR = "${config.xdg.dataHome}/password-store";
        };
        ExecStart = "${cfg.package}/bin/nc-bookmark-sync ${
            tomlFormat.generate "nc-bookmark-sync-config.toml" cfg.config
          }";
      };
    };

    systemd.user.timers.nc-bookmark-sync = {
      Unit = { Description = "Nextcloud Bookmark sync timer."; };
      Timer = {
        Unit = "nc-bookmark-sync.service";
        OnCalendar = cfg.frequency;
        Persistent = true;
        AccuracySec = "1h";
        RandomizedDelaySec = "1h";
      };
      Install = { WantedBy = [ "timers.target" ]; };
    };

  };
}
