{ pkgs, ... }:
{
  enable = false;
  deps = [ pkgs.swww ];
  services.swww-daemon = {
    description = "swww wallpaper daemon";
    wantedBy = [ "graphical-session.target" ];
    partOf = [ "graphical-session.target" ];
    after = [ "graphical-session.target" ];
    serviceConfig = {
      ExecStart = "${pkgs.swww}/bin/swww-daemon";
      ExecStartPost = "${pkgs.bash}/bin/bash -c 'sleep 1 && cyberdeck wallpaper init'";
      Restart = "on-failure";
      RestartSec = 2;
    };
    environment = {
      WAYLAND_DISPLAY = "wayland-1";
    };
  };
}
