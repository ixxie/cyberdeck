{ pkgs, ... }:
{
  enable = false;
  deps = [ pkgs.wl-clipboard pkgs.cliphist ];
}
