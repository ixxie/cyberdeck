{ pkgs, lib, ... }:
{
  enable = false;
  name = "Profiles";
  icon = "scales";
  type = "actions";
  deps = [ pkgs.power-profiles-daemon ];
  source = {
    type = "poll";
    command = [ "sh" "-c" ''printf '{"profile":"%s"}' "$(powerprofilesctl get)"'' ];
    interval = 5;
  };
  badges = {
    default = {
      template = ''{% if profile == "power-saver" %}{{ "leaf" | icon }}{% elif profile == "performance" %}{{ "lightning" | icon }}{% else %}{{ "scales" | icon }}{% endif %}'';
      condition = ''{{ profile != "balanced" }}'';
    };
  };
  key-hints = [
    { key = "s"; action = "powerprofilesctl set power-saver"; label = "saver"; icon = "leaf"; }
    { key = "b"; action = "powerprofilesctl set balanced"; label = "balanced"; icon = "scales"; }
    { key = "p"; action = "powerprofilesctl set performance"; label = "performance"; icon = "lightning"; }
    { key = "Esc"; action = "back"; label = "back"; }
  ];
}
