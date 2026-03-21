{ pkgs, lib, ... }:
{
  enable = false;
  name = "Wallpaper";
  icon = "image";
  type = "wallpaper";
  deps = [ pkgs.swww ];
  params = {
    dir = "~/Pictures/Wallpapers";
    fill = "crop";
    transition = "fade";
    transition-duration = "1";
  };
  source = {
    type = "native";
    kind = "wallpaper";
    interval = 30;
  };
  commands = {
    shuffle = "cyberdeck wallpaper shuffle";
    init = "cyberdeck wallpaper init";
  };
  key-hints = [
    { key = "s"; action = "cyberdeck wallpaper shuffle"; label = "shuffle"; }
    { key = "Esc"; action = "back"; label = "back"; }
  ];
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
