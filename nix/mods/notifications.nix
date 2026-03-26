{ pkgs, ... }:
{
  enable = true;
  deps = [ pkgs.libnotify ];
}
