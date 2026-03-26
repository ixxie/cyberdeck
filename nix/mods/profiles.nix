{ pkgs, ... }:
{
  enable = false;
  deps = [ pkgs.power-profiles-daemon ];
}
