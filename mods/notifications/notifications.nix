{ pkgs, lib, ... }:
{
  enable = false;
  name = "Notifications";
  icon = "bell";
  deps = [ pkgs.swaynotificationcenter ];
  source = {
    type = "native";
    kind = "notifications";
    interval = 5;
  };
  badges = {
    default = {
      template = ''{{ "bell" | icon }}'';
      condition = "{{ count > 0 }}";
    };
  };
  widget = {
    template = ''{% if count == 0 %}no notifications{% elif count == 1 %}1 notification{% else %}{{ count }} notifications{% endif %}{% if dnd %} (DnD){% endif %}'';
  };
  key-hints = [
    { key = "d"; action = "swaync-client -d"; label = "DnD"; }
    { key = "c"; action = "swaync-client -C"; label = "clear"; }
    { key = "Esc"; action = "back"; label = "back"; }
  ];
}
