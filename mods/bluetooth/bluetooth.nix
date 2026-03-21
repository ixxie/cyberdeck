{ pkgs, lib, ... }:
{
  enable = false;
  name = "Bluetooth";
  icon = "bluetooth";
  type = "bluetooth";
  deps = [ pkgs.bluez ];
  source = {
    type = "native";
    kind = "bluetooth";
    interval = 10;
  };
  badges = {
    default = {
      template = ''{{ "bluetooth" | icon }}'';
      condition = "{{ devices | selectattr('connected', 'true') | list | length > 0 }}";
    };
  };
  widget = {
    template = ''{% if not powered %}off{% elif devices | length == 0 %}no devices{% else %}{% for d in devices %}{% if d.connected %}{{ d.name }}{% else %}{{ d.name | dim }}{% endif %}{% if not loop.last %}  {% endif %}{% endfor %}{% endif %}'';
  };
  key-hints = [
    { key = "Esc"; action = "back"; label = "back"; }
  ];
}
