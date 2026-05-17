{ pkgs, ... }:
{
  enable = false;
  deps = [ pkgs.wireplumber pkgs.rnnoise-plugin ];
  env.LADSPA_PATH = "${pkgs.rnnoise-plugin}/lib";
}
