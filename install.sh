#!/bin/sh
set -e

REPO="ixxie/cyberdeck"
INSTALL_DIR="$HOME/.local"
BIN_DIR="$INSTALL_DIR/bin"
DATA_DIR="$INSTALL_DIR/share/cyberdeck"
ICONS_DIR="$DATA_DIR/icons"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/cyberdeck"
SERVICE_DIR="$HOME/.config/systemd/user"

ARCH=$(uname -m)
case "$ARCH" in
    x86_64) ARCH_TAG="x86_64" ;;
    aarch64) ARCH_TAG="aarch64" ;;
    *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

# Find latest release
if command -v curl >/dev/null 2>&1; then
    FETCH="curl -fsSL"
    FETCH_OUT="curl -fsSL -o"
elif command -v wget >/dev/null 2>&1; then
    FETCH="wget -qO-"
    FETCH_OUT="wget -qO"
else
    echo "Error: curl or wget required"
    exit 1
fi

echo "==> Finding latest release..."
LATEST=$($FETCH "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
if [ -z "$LATEST" ]; then
    echo "Error: could not determine latest release"
    exit 1
fi
echo "    Version: $LATEST"

TARBALL="cyberdeck-${ARCH_TAG}-linux.tar.gz"
URL="https://github.com/$REPO/releases/download/$LATEST/$TARBALL"

# Download and extract
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "==> Downloading $TARBALL..."
$FETCH_OUT "$TMPDIR/$TARBALL" "$URL"

echo "==> Extracting..."
tar xzf "$TMPDIR/$TARBALL" -C "$TMPDIR"

# Install binary
mkdir -p "$BIN_DIR"
cp "$TMPDIR/cyberdeck" "$BIN_DIR/cyberdeck"
chmod +x "$BIN_DIR/cyberdeck"
ln -sf "$BIN_DIR/cyberdeck" "$BIN_DIR/deck"
echo "    Installed: $BIN_DIR/cyberdeck"

# Install icons
if [ -d "$TMPDIR/icons" ]; then
    mkdir -p "$ICONS_DIR"
    cp -r "$TMPDIR/icons/"* "$ICONS_DIR/"
    echo "    Icons: $ICONS_DIR"
fi

# Default config
if [ ! -f "$CONFIG_DIR/config.json" ]; then
    mkdir -p "$CONFIG_DIR"
    cat > "$CONFIG_DIR/config.json" << 'EOF'
{
  "settings": {},
  "bar": {
    "modules": {
      "calendar": {},
      "workspaces": {},
      "window": {},
      "notifications": {}
    }
  }
}
EOF
    echo "    Config: $CONFIG_DIR/config.json"
else
    echo "    Config: existing (kept)"
fi

# Systemd service
mkdir -p "$SERVICE_DIR"
cp "$TMPDIR/cyberdeck.service" "$SERVICE_DIR/cyberdeck.service" 2>/dev/null \
    || cat > "$SERVICE_DIR/cyberdeck.service" << 'EOF'
[Unit]
Description=Cyberdeck desktop shell
PartOf=graphical-session.target
After=graphical-session.target

[Service]
ExecStart=%h/.local/bin/cyberdeck
Restart=on-failure
RestartSec=2
Environment=WAYLAND_DISPLAY=wayland-1
Environment=RUST_LOG=cyberdeck=info

[Install]
WantedBy=graphical-session.target
EOF
echo "    Service: $SERVICE_DIR/cyberdeck.service"

echo ""
echo "==> Installation complete!"
echo ""
echo "To start cyberdeck:"
echo "  systemctl --user daemon-reload"
echo "  systemctl --user enable --now cyberdeck"
echo ""
echo "To add modules, edit $CONFIG_DIR/config.json"
echo "and install their dependencies:"
echo ""

# Detect package manager and print deps
if command -v apt >/dev/null 2>&1; then
    PKG="sudo apt install"
    echo "  Runtime (required):"
    echo "    $PKG libwayland-client0 libxkbcommon0 libfontconfig1"
    echo ""
    echo "  Module deps (install as needed):"
    echo "    bluetooth:   $PKG bluez"
    echo "    brightness:  $PKG brightnessctl"
    echo "    media:       $PKG playerctl"
    echo "    network:     $PKG network-manager"
    echo "    outputs:     $PKG wireplumber"
    echo "    inputs:      $PKG wireplumber"
    echo "    weather:     $PKG curl"
    echo "    snip:        $PKG grim slurp wl-clipboard"
    echo "    wallpaper:   (install swww from source)"
elif command -v pacman >/dev/null 2>&1; then
    PKG="sudo pacman -S"
    echo "  Runtime (required):"
    echo "    $PKG wayland libxkbcommon fontconfig"
    echo ""
    echo "  Module deps (install as needed):"
    echo "    bluetooth:   $PKG bluez-utils"
    echo "    brightness:  $PKG brightnessctl"
    echo "    media:       $PKG playerctl"
    echo "    network:     $PKG networkmanager"
    echo "    outputs:     $PKG wireplumber"
    echo "    inputs:      $PKG wireplumber"
    echo "    weather:     $PKG curl"
    echo "    snip:        $PKG grim slurp wl-clipboard wl-screenrec"
    echo "    wallpaper:   $PKG swww"
elif command -v dnf >/dev/null 2>&1; then
    PKG="sudo dnf install"
    echo "  Runtime (required):"
    echo "    $PKG wayland libxkbcommon fontconfig"
    echo ""
    echo "  Module deps (install as needed):"
    echo "    bluetooth:   $PKG bluez"
    echo "    brightness:  $PKG brightnessctl"
    echo "    media:       $PKG playerctl"
    echo "    network:     $PKG NetworkManager"
    echo "    outputs:     $PKG wireplumber"
    echo "    inputs:      $PKG wireplumber"
    echo "    weather:     $PKG curl"
else
    echo "  See dist/deps.md for per-module dependencies."
fi
