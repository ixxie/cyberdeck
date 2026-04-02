{
  description = "Retrobar - a terminal-aesthetic Wayland bar";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    phosphor-icons = {
      url = "github:phosphor-icons/core";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, crane, flake-utils, phosphor-icons }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        craneLib = crane.mkLib pkgs;

        nativeBuildInputs = with pkgs; [
          pkg-config
        ];

        buildInputs = with pkgs; [
          wayland
          libxkbcommon
          fontconfig
          freetype
          vulkan-loader
          libGL
        ];

        extraFilter = path: _type:
          (builtins.match ".*\\.json$" path != null) ||
          (builtins.match ".*\\.mod\\.toml$" path != null);
        srcFilter = path: type:
          (extraFilter path type) || (craneLib.filterCargoSources path type);
        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = srcFilter;
        };

        cyberdeck = craneLib.buildPackage {
          inherit src nativeBuildInputs buildInputs;
        };
      in
      {
        packages.default = cyberdeck;

        devShells.default = craneLib.devShell {
          inputsFrom = [ cyberdeck ];
          packages = with pkgs; [
            rust-analyzer
            cargo-watch
          ];
        };
      }
    ) // {
      nixosModules.default = import ./nix/module.nix {
        flake = self;
        inherit phosphor-icons;
      };
    };
}
