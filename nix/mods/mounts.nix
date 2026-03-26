{ pkgs, ... }:
{
  enable = false;
  deps = [ pkgs.udisks2 pkgs.jq ];
}
