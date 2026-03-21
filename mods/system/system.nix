{ pkgs, lib, ... }:
{
  enable = false;
  name = "System";
  icon = "cpu";
  deps = [];
  source = {
    type = "native";
    kind = "system";
    interval = 5;
  };
  badges = {
    default = {
      template = ''{{ "cpu" | icon }}'';
      condition = "{{ cpu_percent > 90 or temp > 90 }}";
    };
  };
  widget = {
    template = ''{{ cpu_percent }}%  {{ "memory" | icon }} {{ mem_used_bytes | human_bytes }}/{{ mem_total_bytes | human_bytes }}{% if temp %}  {{ temp }}°C{% endif %}'';
  };
  key-hints = [
    { key = "Esc"; action = "back"; label = "back"; }
  ];
}
