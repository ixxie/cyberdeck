{ pkgs, ... }:
{
  enable = false;
  deps = [ pkgs.wireplumber pkgs.rnnoise-plugin ];
}
