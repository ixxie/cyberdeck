{ pkgs, ... }:
{
  enable = false;
  deps = [
    pkgs.grim
    pkgs.slurp
    pkgs.wl-clipboard
    pkgs.xdg-user-dirs
  ];
}
