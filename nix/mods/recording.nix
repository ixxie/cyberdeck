{ pkgs, ... }:
{
  enable = false;
  deps = [
    pkgs.slurp
    pkgs.wl-screenrec
    pkgs.xdg-user-dirs
  ];
}
