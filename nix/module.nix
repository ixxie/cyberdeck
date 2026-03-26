{ flake, phosphor-icons }:
{ config, lib, pkgs, ... }:

let
  cfg = config.services.cyberdeck;
  defaults = import ./mods { inherit pkgs lib; };

  # Merge user overrides into module definitions
  mergedModules = lib.recursiveUpdate defaults.modules (cfg.mods or {});

  # Filter to enabled modules
  enabledModules = lib.filterAttrs
    (_name: mod: mod.enable or false)
    mergedModules;

  # Collect deps from enabled modules
  moduleDeps = lib.concatLists (
    lib.mapAttrsToList (_name: mod: mod.deps or []) enabledModules
  );

  # Collect services from enabled modules
  moduleServices = lib.foldlAttrs (acc: _name: mod:
    acc // (mod.services or {})
  ) {} enabledModules;

  # Collect environment variables from enabled modules
  moduleEnv = lib.foldlAttrs (acc: _name: mod:
    acc // (mod.env or {})
  ) {} enabledModules;

  # Collect module param overrides for the sparse JSON config
  moduleOverrides = lib.filterAttrs (_: v: v != {}) (
    lib.mapAttrs (_name: mod:
      removeAttrs mod [ "enable" "deps" "services" ]
    ) enabledModules
  );

  # Sparse config: settings + order + only module overrides (params, etc.)
  finalConfig = {
    settings = defaults.settings // (cfg.settings or {}) // {
      icons-dir = "${phosphor-icons}/assets";
    };
    bar = {
      modules = moduleOverrides;
    };
  };

  depsPath = lib.makeBinPath moduleDeps;

  configFile = pkgs.writeText "cyberdeck-config.json"
    (builtins.toJSON finalConfig);

  cyberdeckUnwrapped = flake.packages.${pkgs.system}.default;

  cyberdeckPkg = pkgs.runCommand "cyberdeck-wrapped" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
  } ''
    mkdir -p $out/bin
    makeWrapper ${cyberdeckUnwrapped}/bin/cyberdeck $out/bin/cyberdeck \
      --prefix PATH : "${depsPath}"
    ln -s $out/bin/cyberdeck $out/bin/deck
  '';
in
{
  options.services.cyberdeck = {
    enable = lib.mkEnableOption "cyberdeck Wayland bar";

    settings = lib.mkOption {
      type = lib.types.attrs;
      default = {};
      description = "Settings override for cyberdeck (merged with defaults)";
    };

    mods = lib.mkOption {
      type = lib.types.attrs;
      default = {};
      description = "Module overrides (enable, params, etc.)";
    };

    extraConfig = lib.mkOption {
      type = lib.types.attrs;
      default = {};
      description = "Full config override (merged recursively with default config)";
    };

  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cyberdeckPkg ];

    systemd.user.services = (lib.mapAttrs (_name: svc:
      svc // { path = (svc.path or []) ++ [ cyberdeckPkg ]; }
    ) moduleServices) // {
      cyberdeck = {
      description = "Cyberdeck desktop shell";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];

      path = [ "/run/current-system/sw" ] ++ moduleDeps;

      serviceConfig = {
        ExecStart = "${cyberdeckPkg}/bin/cyberdeck --config ${configFile}";
        Restart = "on-failure";
        RestartSec = 2;
      };

      environment = moduleEnv // {
        WAYLAND_DISPLAY = "wayland-1";
        RUST_LOG = "cyberdeck=debug";
      };

      preStart = ''
        mkdir -p ''${XDG_CONFIG_HOME:-$HOME/.config}/cyberdeck
        ln -sf ${configFile} ''${XDG_CONFIG_HOME:-$HOME/.config}/cyberdeck/config.json
      '';
    };
    };

    system.activationScripts.restart-cyberdeck = ''
      for uid_dir in /run/user/*; do
        uid="''${uid_dir##*/}"
        user=$(${pkgs.coreutils}/bin/id -nu "$uid" 2>/dev/null) || continue
        XDG_RUNTIME_DIR="$uid_dir" \
          ${pkgs.util-linux}/bin/runuser -u "$user" -- \
          ${pkgs.systemd}/bin/systemctl --user daemon-reload 2>/dev/null || true
        XDG_RUNTIME_DIR="$uid_dir" \
          ${pkgs.util-linux}/bin/runuser -u "$user" -- \
          ${pkgs.systemd}/bin/systemctl --user restart cyberdeck.service 2>/dev/null || true
      done
    '';
  };
}
