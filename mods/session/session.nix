{ pkgs, lib, ... }:
{
  enable = false;
  name = "Session";
  icon = "power";
  type = "actions";
  deps = [ pkgs.upower ];
  source = {
    type = "native";
    kind = "session";
    interval = 10;
  };
  badges = {
    default = {
      template = ''{% if charging %}{{ "battery-charging" | icon }}{% elif capacity < 10 %}{{ "battery-empty" | icon }}{% elif capacity < 35 %}{{ "battery-low" | icon }}{% elif capacity < 65 %}{{ "battery-half" | icon }}{% else %}{{ "battery-full" | icon }}{% endif %}'';
      condition = "{{ capacity < 20 and not charging }}";
    };
  };
  hooks = [
    { condition = "{{ capacity < 20 and not charging }}"; action = "notify-send -u normal 'Power' 'Battery at {{ capacity }}%'"; timeout = 5; }
    { condition = "{{ capacity < 5 and not charging }}"; action = "notify-send -u critical 'Power' 'Battery critical'"; timeout = 5; }
  ];
  key-hints = [
    { key = "z"; action = "systemctl suspend"; label = "suspend"; icon = "moon"; }
    { key = "x"; action = "shutdown now"; label = "shutdown"; icon = "power"; }
    { key = "r"; action = "shutdown -r now"; label = "reboot"; icon = "arrow-clockwise"; }
    { key = "l"; action = "niri msg action quit"; label = "logout"; icon = "sign-out"; }
    { key = "Esc"; action = "back"; label = "back"; }
  ];
}
