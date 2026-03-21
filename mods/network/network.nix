{ pkgs, lib, ... }:
{
  enable = false;
  name = "Network";
  icon = "wifi-high";
  deps = [ pkgs.networkmanager ];
  source = {
    type = "native";
    kind = "network";
    interval = 10;
  };
  badges = {
    default = {
      template = ''{% if connected %}{% if signal > 75 %}{{ "wifi-high" | icon }}{% elif signal > 50 %}{{ "wifi-medium" | icon }}{% elif signal > 25 %}{{ "wifi-low" | icon }}{% else %}{{ "wifi-none" | icon }}{% endif %}{% else %}{{ "wifi-slash" | icon }}{% endif %}'';
      condition = "{{ not connected or signal < 25 }}";
    };
  };
  widget = {
    template = ''{% if connected %}{{ ssid }}  {{ signal }}%  {{ ip }}{% else %}disconnected{% endif %}'';
  };
  key-hints = [
    { key = "Esc"; action = "back"; label = "back"; }
  ];
}
