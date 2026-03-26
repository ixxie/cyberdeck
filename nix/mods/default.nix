{ pkgs, lib }:
let
  importMod = name: import ./${name}.nix { inherit pkgs lib; };
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

  bar.order = [
    "notifications" "media" "system" "storage"
    "audio" "brightness" "bluetooth" "weather"
    "network" "profiles" "session" "wallpaper" "window"
    "keyboard" "clipboard" "mounts"
    "calendar" "workspaces"
  ];

  modules = {
    audio = importMod "audio";
    bluetooth = importMod "bluetooth";
    brightness = importMod "brightness";
    calendar = importMod "calendar";
    clipboard = importMod "clipboard";
    keyboard = importMod "keyboard";

    media = importMod "media";
    mounts = importMod "mounts";
    network = importMod "network";
    notifications = importMod "notifications";
    profiles = importMod "profiles";
    session = importMod "session";
    storage = importMod "storage";
    system = importMod "system";
    wallpaper = importMod "wallpaper";
    weather = importMod "weather";
    window = importMod "window";
    workspaces = importMod "workspaces";
  };
}
