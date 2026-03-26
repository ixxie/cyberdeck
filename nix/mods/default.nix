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

  modules = {
    bluetooth = importMod "bluetooth";
    brightness = importMod "brightness";
    calendar = importMod "calendar";
    clipboard = importMod "clipboard";
    keyboard = importMod "keyboard";

    inputs = importMod "inputs";
    mounts = importMod "mounts";
    network = importMod "network";
    notifications = importMod "notifications";
    media = importMod "media";
    outputs = importMod "outputs";
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
