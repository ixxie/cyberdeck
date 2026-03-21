{ pkgs, lib, ... }:
{
  enable = false;
  name = "Mounts";
  icon = "usb";
  deps = [ pkgs.udisks2 pkgs.jq ];
  source = {
    type = "poll";
    command = [ "sh" "-c" ''
      udisksctl status 2>/dev/null | tail -n +3 | while read -r line; do
        dev=$(echo "$line" | awk '{print $NF}')
        [ -z "$dev" ] && continue
        echo "$dev"
      done | sort -u | head -20 | {
        first=true
        printf '{"devices":['
        while read -r dev; do
          info=$(udisksctl info -b "/dev/$dev" 2>/dev/null)
          mount=$(echo "$info" | grep MountPoints | awk '{print $2}')
          label=$(echo "$info" | grep IdLabel | awk '{print $2}')
          size=$(echo "$info" | grep Size | head -1 | awk '{print $2}')
          removable=$(echo "$info" | grep Removable | head -1 | awk '{print $2}')
          [ "$removable" != "true" ] && continue
          [ "$first" = true ] && first=false || printf ','
          printf '{"dev":"%s","label":"%s","mount":"%s","size":%s}' \
            "$dev" "${label:-$dev}" "${mount:--}" "${size:-0}"
        done
        printf ']}'
      }
    '' ];
    interval = 10;
  };
  badges = {
    default = {
      template = ''{{ "usb" | icon }}'';
      condition = "{{ devices | length > 0 }}";
    };
  };
  widget = {
    template = ''{% if devices | length == 0 %}no removable devices{% else %}{% for d in devices %}{{ d.label }}  {{ d.mount }}{% if d.size > 0 %}  {{ d.size | human_bytes }}{% endif %}{% if not loop.last %}  {% endif %}{% endfor %}{% endif %}'';
  };
  key-hints = [
    { key = "Esc"; action = "back"; label = "back"; }
  ];
}
