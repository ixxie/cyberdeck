{ pkgs, lib, ... }:
{
  enable = false;
  name = "Media";
  icon = "play";
  deps = [ pkgs.playerctl ];
  source = {
    type = "native";
    kind = "media";
    interval = 1;
  };
  badges = {
    default = {
      template = ''{% if status == "Playing" %}{{ "play" | icon }}{% else %}{{ "pause" | icon }}{% endif %}'';
      condition = ''{{ status == "Playing" }}'';
    };
  };
  widget = {
    template = ''{% if title %}{{ title }}{% if artist %} — {{ artist }}{% endif %}{% else %}no media{% endif %}'';
  };
  key-hints = [
    { key = "p"; action = "playerctl play-pause"; label = "play/pause"; }
    { key = "["; action = "playerctl previous"; label = "prev"; }
    { key = "]"; action = "playerctl next"; label = "next"; }
    { key = "Esc"; action = "back"; label = "back"; }
  ];
}
