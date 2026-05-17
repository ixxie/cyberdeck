{ pkgs, lib }:
let
  importMod = name: import ./${name}.nix { inherit pkgs lib; };
in
{
  settings = {
    position = "top";
    font = "monospace";
    font-size = 14;
    icon-weight = "duotone";
  };

  modules = {
    audio = importMod "audio";
    bluetooth = importMod "bluetooth";
    brightness = importMod "brightness";
    calendar = importMod "calendar";
    clipboard = importMod "clipboard";
    keyboard = importMod "keyboard";

    mounts = importMod "mounts";
    network = importMod "network";
    notifications = importMod "notifications";
    media = importMod "media";
    profiles = importMod "profiles";
    session = importMod "session";
    screenshot = importMod "screenshot";
    recording = importMod "recording";
    storage = importMod "storage";
    system = importMod "system";
    wallpaper = importMod "wallpaper";
    weather = importMod "weather";
    window = importMod "window";
    workspaces = importMod "workspaces";
  };
}
