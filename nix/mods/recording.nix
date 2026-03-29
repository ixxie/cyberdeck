{ pkgs, ... }:
{
  enable = false;
  deps = [
    pkgs.slurp
    pkgs.wl-screenrec
  ];
}
