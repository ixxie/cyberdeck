{ ... }:
{
  # Reads /sys/class/rfkill and writes /dev/rfkill directly — the uaccess ACL
  # on the device covers the logged-in user, so no tools or helpers needed.
  enable = false;
}
