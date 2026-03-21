{ pkgs, lib, ... }:
{
  enable = false;
  name = "Keyboard";
  icon = "keyboard";
  type = "actions";
  deps = [ pkgs.sway ];
  source = {
    type = "poll";
    command = [ "sh" "-c" ''
      layout=$(swaymsg -t get_inputs | jq -r '[.[] | select(.type == "keyboard") | .xkb_active_layout_name] | first // "unknown"')
      printf '{"layout":"%s"}' "$layout"
    '' ];
    interval = 5;
  };
  badges = {
    default = {
      template = ''{{ "keyboard" | icon }}'';
      condition = ''{{ layout != "English (US)" and layout != "unknown" }}'';
    };
  };
  widget = {
    template = ''{{ layout }}'';
  };
  key-hints = [
    { key = "n"; action = "swaymsg input type:keyboard xkb_switch_layout next"; label = "next"; }
    { key = "Esc"; action = "back"; label = "back"; }
  ];
}
