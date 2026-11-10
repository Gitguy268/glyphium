#!/bin/sh
# Flatpak launcher. With no -e, the session runs your login shell on the host through
# host-spawn (which gives it a real terminal there), not the sandbox's own sh.
for arg in "$@"; do
  [ "$arg" = "-e" ] && exec /app/libexec/glyphium "$@"
done
exec /app/libexec/glyphium "$@" -e /app/bin/host-spawn
