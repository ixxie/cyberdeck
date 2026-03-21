{ pkgs, lib, ... }:
{
  enable = false;
  name = "Clipboard";
  icon = "clipboard-text";
  type = "actions";
  deps = [ pkgs.wl-clipboard pkgs.cliphist ];
  source = {
    type = "poll";
    command = [ "sh" "-c" ''
      count=$(cliphist list 2>/dev/null | wc -l)
      preview=$(cliphist list 2>/dev/null | head -1 | cut -c1-60)
      printf '{"count":%d,"preview":"%s"}' "$count" "$preview"
    '' ];
    interval = 5;
  };
  badges = {
    default = {
      template = ''{{ "clipboard-text" | icon }}'';
      condition = "{{ count > 0 }}";
    };
  };
  widget = {
    template = ''{{ count }} entries{% if preview %}  {{ preview | dim }}{% endif %}'';
    condition = "{{ count > 0 }}";
  };
  key-hints = [
    { key = "p"; action = "cliphist list | head -1 | cliphist decode | wl-copy"; label = "paste last"; }
    { key = "d"; action = "cliphist wipe"; label = "clear"; }
    { key = "Esc"; action = "back"; label = "back"; }
  ];
}
