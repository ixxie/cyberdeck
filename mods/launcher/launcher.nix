{ pkgs, lib, ... }:
{
  enable = false;
  name = "Launcher";
  icon = "terminal";
  source = {
    type = "native";
    kind = "launcher";
    interval = 60;
  };
  badges = {
    default = {
      template = ''{{ "terminal" | icon }}'';
      icon-scale = 0.7;
    };
  };
  widget = {
    template = ''{{ "terminal" | icon }}'';
  };
}
