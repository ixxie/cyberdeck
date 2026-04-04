{ pkgs, ... }:
{
  enable = false;
  deps = [ pkgs.brightnessctl ];
  systemServices.brightness-boot-reset = {
    description = "Reset screen brightness to 100% before greeter";
    wantedBy = [ "display-manager.service" ];
    before = [ "display-manager.service" ];
    serviceConfig = {
      Type = "oneshot";
      ExecStart = "${pkgs.brightnessctl}/bin/brightnessctl set 100%";
    };
  };
}
