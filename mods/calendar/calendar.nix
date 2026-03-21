{ pkgs, lib, ... }:
{
  enable = false;
  name = "Calendar";
  icon = "calendar";
  deps = [];
  type = "calendar";
  source = {
    type = "native";
    kind = "calendar";
    interval = 1;
  };
  badges = {
    default = {
      template = "{{ hour }}:{{ minute }}";
    };
  };
}
