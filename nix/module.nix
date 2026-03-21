{ flake, phosphor-icons }:
{ config, lib, pkgs, ... }:

let
  cfg = config.services.cyberdeck;
  defaultConfig = import ../mods { inherit pkgs lib; };

  # Merge user mods into default config
  modsOverride = if cfg.mods != {} then { bar.modules = cfg.mods; } else {};
  mergedConfig = lib.recursiveUpdate
    (lib.recursiveUpdate defaultConfig modsOverride)
    (cfg.extraConfig or {});

  # Recursively strip Nix-only fields from the module tree
  stripModule = mod:
    let
      stripped = removeAttrs mod [ "enable" "deps" "services" ];
      hasSubs = stripped ? modules && builtins.isAttrs stripped.modules;
      strippedSubs = if hasSubs then
        lib.mapAttrs (_name: stripModule) (
          lib.filterAttrs (_name: child: child.enable or false) stripped.modules
        )
      else {};
    in stripped // (if hasSubs then { modules = strippedSubs; } else {});

  # Filter enabled modules, then strip recursively
  barModules = lib.filterAttrs
    (_name: mod: mod.enable or false)
    mergedConfig.bar.modules;

  strippedBar = (removeAttrs mergedConfig.bar [ "enable" "deps" ]) // {
    modules = lib.mapAttrs (_name: stripModule) barModules;
  };

  finalConfig = {
    settings = mergedConfig.settings // (cfg.settings or {}) // {
      icons-dir = "${phosphor-icons}/assets";
    };
    bar = strippedBar;
  };

  # Collect deps recursively from enabled modules
  collectDeps = mod:
    let
      ownDeps = mod.deps or [];
      subDeps = if mod ? modules && builtins.isAttrs mod.modules then
        lib.concatLists (
          lib.mapAttrsToList (_name: child:
            if child.enable or false then collectDeps child else []
          ) mod.modules
        )
      else [];
    in ownDeps ++ subDeps;

  moduleDeps = lib.concatLists (
    lib.mapAttrsToList (_name: mod:
      if mod.enable or false then collectDeps mod else []
    ) mergedConfig.bar.modules
  );

  # Collect services recursively from enabled modules
  collectServices = mod:
    let
      ownServices = mod.services or {};
      subServices = if mod ? modules && builtins.isAttrs mod.modules then
        lib.foldlAttrs (acc: _name: child:
          if child.enable or false then acc // (collectServices child) else acc
        ) {} mod.modules
      else {};
    in ownServices // subServices;

  rawModuleServices = lib.foldlAttrs (acc: _name: mod:
    if mod.enable or false then acc // (collectServices mod) else acc
  ) {} mergedConfig.bar.modules;

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
    ) rawModuleServices) // {
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

      environment = {
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
