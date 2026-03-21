{ pkgs, lib, ... }:
{
  enable = false;
  name = "Audio";
  icon = "speaker-high";
  deps = [ pkgs.wireplumber ];
  source = {
    type = "native";
    kind = "audio";
    interval = 1;
  };
  badges = {
    default = {
      template = ''{% if muted %}{{ "speaker-slash" | icon }}{% else %}{{ "speaker-high" | icon }}{% endif %}'';
      condition = "{{ muted }}";
    };
  };
  widget = {
    template = ''{% if muted %}muted{% else %}{{ volume | meter(max=100, width=20) }}{% endif %}'';
  };
  hooks = [
    { condition = ''{{ changed(key="volume") or changed(key="muted") }}''; action = "spotlight"; timeout = 3; }
  ];
  key-hints = [
    { key = "Up"; action = "wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%+"; label = "vol+"; }
    { key = "Down"; action = "wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-"; label = "vol-"; }
    { key = "m"; action = "wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle"; label = "mute"; }
    { key = "Esc"; action = "back"; label = "back"; }
  ];
}
