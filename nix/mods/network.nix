{ pkgs, ... }:
{
  enable = false;
  deps = [ pkgs.networkmanager ];
}
