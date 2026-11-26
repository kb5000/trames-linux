# Linux SHM desktop and input

`--desktop x11` enables the shared Runtime Wayland compositor and presents each
visible guest toplevel in an original host X11 window. Native Wayland clients
use `XDG_RUNTIME_DIR=/run/user/0` and `WAYLAND_DISPLAY=wayland-0`. Defaults also
include `HOME=/root`, `LANG=C.UTF-8` and `GDK_BACKEND=wayland`; explicit `--setenv`
values take precedence. This path does not require Vulkan or initialize a GPU.

```sh
trames run --rootfs /path/to/rootfs --desktop x11 --cpus 4 -- /usr/bin/weston-terminal
```

The rootfs needs its original ARM64 applications and libraries. The foreground
adapter mounts private tmpfs at `/run/user/0` and `/tmp` before enabling
the compositor. Guest socket inodes stay in the shared VFS, including when the
rootfs is a writable host directory. The desktop session has a fresh temporary directory, including X11 lock
files and sockets; underlying rootfs `/tmp` data is retained underneath the
mount and does not appear in that session. Other `/run` data stays visible.
Managed stores use the same session IPC setup. A session devpts instance
backs `/dev/pts` and GUI-created PTYs.

## Xwayland

Build the small GPLv3 launcher as a static guest executable (or use the shared
installer’s existing musl-loader ABI):

```sh
musl-gcc -static -O2 -Wall -Wextra -Werror tools/xwayland-launcher.c -o trames-xwayland
trames run --rootfs /path/to/rootfs --desktop x11 \
  --xwayland-launcher ./trames-xwayland -- /bin/sh
```

Then, inside that session:

```sh
trames-xwayland :0 >/tmp/xwayland.log 2>&1 &
export DISPLAY=:0
xterm -fa monospace
```

The launcher source is retained from Trames iPad's
`accelerator/xwayland/launcher.c`; Linux uses the same authenticated XWM protocol.
`--xwayland-launcher` installs it through the existing shared distro installer.
Xwayland, fonts, xkbcomp, XKB data and X11 applications remain original distro
packages. The X server starts explicitly and uses SHM when Vulkan is disabled.
`--setenv GDK_BACKEND=x11` selects X11 for toolkits that otherwise prefer Wayland.

## Presentation, readiness and ownership

The frontend subscribes to shared compositor snapshots before its deciding pass
and watches the native XCB connection in the original foreground `HostWait`.
XCB can buffer events during a reply: the presenter retains a pending event and
latches that same host wake, so sleeping cannot lose already-buffered input.
Compositor command backpressure retains unsent commands and subscribes to the
existing command-space notification. No window/service thread or retry timer is
added. The existing shared compositor keeps its existing protocol worker.

Snapshots retain the original SHM image leases through software composition.
The presenter handles scene positions, popup bounds, viewport crops, buffer
scale/transform and premultiplied alpha into an X11 RGB image. It uses raw,
bounded reads from retained mappings, without constructing Rust references to
guest-writable pixels. Invalid concurrent guest writes may alter pixels; they
cannot replace the retained mapping or expand its range. Initial supported CPU
hosts are little-endian ARM64 and AMD64. CPU page geometry is independent of
image row stride and X11 request chunking.

PutImage is a software fallback with a CPU copy. A native server roundtrip proves
that image requests were processed before acknowledging the shared frame. This
is not a vblank or accelerated-GPU presentation claim. Expose repaints the last
retained scene. Resize negotiates guest content dimensions; title changes and
guest window removal update the original host window. A host close requests
closure of that guest toplevel; the CLI/session can continue after GUI exit.

Keyboard input currently uses Xorg/Xwayland's evdev keycodes and the shared US
keymap. Pointer coordinates, buttons, wheel and keyboard modifiers are translated
into the existing `InputEvent` API. Focus loss clears held keys/buttons through
the shared seat. Clipboard, host IME/layout negotiation, full EWMH window state,
interactive client-side move/resize gestures and a native host Wayland frontend
remain follow-up work.

The original `--display x11` fixed Vulkan display remains a separate mode. It
cannot be combined with `--desktop`; `--display-size` applies only to that fixed
display. GPU-backed desktop images report unsupported rather than silently
claiming acceleration. DMA-BUF/fence import, Zink desktop, physical audio/video
and same-host accelerated KVM acceptance remain separate gates.

## Automated acceptance

`python3 scripts/validate-desktop.py --native-only --unit-binary UNIT --output OUT`
uses original libwayland and the production desktop frontend without a guest
CPU. Hosted ARM64/AMD64 presentation CI runs this gate. Required host tools are
Xvfb, wayland-scanner, Wayland development/protocol files, XCB and XCB XTEST.

Add `--runner /path/to/trames` and omit `--native-only` on ARM64 Linux with real
KVM. The controller adds both KVM transports and one/four worker cases for
Wayland and original distro Xwayland, managed-store creation/cold reopen, plus
original Weston Terminal and xterm.
Independent X11 probes read actual pixels and inject host keyboard/pointer
input. Logs record focus loss, wheel, resize/expose, close, CLI continuation and
idle CPU. All scratch files live under the explicit output directory; no new VM
or disk image is required. These gates qualify the software desktop only.
