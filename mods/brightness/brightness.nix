{ pkgs, lib, ... }:
{
  enable = false;
  name = "Brightness";
  icon = "sun";
  deps = [ pkgs.brightnessctl ];
  source = {
    type = "native";
    kind = "brightness";
    interval = 1;
  };
  badges = {
    default = {
      template = ''{{ "sun" | icon }}'';
      condition = "{{ brightness < 30 }}";
    };
  };
  widget = {
    template = ''{{ brightness | meter(max=100, width=20) }}'';
  };
  hooks = [
    { condition = ''{{ changed(key="brightness") }}''; action = "spotlight"; timeout = 3; }
  ];
  key-hints = [
    { key = "Up"; action = "brightnessctl set +10%"; label = "brighter"; }
    { key = "Down"; action = "brightnessctl set 10%-"; label = "dimmer"; }
    { key = "Esc"; action = "back"; label = "back"; }
  ];
}
