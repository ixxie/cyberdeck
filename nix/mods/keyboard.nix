{ pkgs, ... }:
{
  enable = true;
  deps = [ pkgs.sway pkgs.jq ];
  params = {
    default = "English (US)";
  };
}
