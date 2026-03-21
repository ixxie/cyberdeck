{ pkgs, lib }:
let
  importMod = name:
    import ./${name}/${name}.nix {
      inherit pkgs lib;
    };
in
{
  settings = {
    position = "top";
    font = "monospace";
    font-size = 14;
    padding = 6;
    padding-horizontal = 12;
    icon-weight = "duotone";
    background = {
      color = "#222222";
      opacity = 0.8;
    };
  };

  bar = {
    order = [
      "notifications" "media" "system" "storage"
      "audio" "brightness" "bluetooth" "weather"
      "network" "profiles" "session" "wallpaper" "window"
      "keyboard" "clipboard" "mounts"
      "calendar" "workspaces"
    ];
    modules = {
      calendar = importMod "calendar";
      workspaces = importMod "workspaces";
      network = importMod "network";
      session = importMod "session";
      profiles = importMod "profiles";
      system = importMod "system";
      audio = importMod "audio";
      bluetooth = importMod "bluetooth";
      brightness = importMod "brightness";
      storage = importMod "storage";
      weather = importMod "weather";
      launcher = importMod "launcher";
      media = importMod "media";
      notifications = importMod "notifications";
      wallpaper = importMod "wallpaper";
      window = importMod "window";
      keyboard = importMod "keyboard";
      clipboard = importMod "clipboard";
      mounts = importMod "mounts";
    };
  };
}
