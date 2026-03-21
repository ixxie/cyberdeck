{ pkgs, lib, ... }:
{
  enable = false;
  name = "Workspaces";
  icon = "hexagon";
  deps = [];
  source = {
    type = "native";
    kind = "workspaces";
    interval = 1;
  };
  badges = {
    default = {
      template = ''{% for ws in workspaces %}{% if ws.output == __output %}{% if ws.active %}{{ "hexagon" | icon }}{% else %}{{ "hexagon" | icon | dim }}{% endif %}{% if not loop.last %} {% endif %}{% endif %}{% endfor %}'';
      icon-scale = 0.7;
    };
  };
}
