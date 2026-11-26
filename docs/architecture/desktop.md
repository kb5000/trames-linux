# Linux native desktop, clipboard, input methods and Vulkan

`--desktop x11` and `--desktop wayland` enable the shared Runtime Wayland
compositor and present each visible guest toplevel in an original native host
window. Select the host session explicitly; guest Wayland/Xwayland selection
is independent of the host frontend. Native Wayland clients
use `XDG_RUNTIME_DIR=/run/user/0` and `WAYLAND_DISPLAY=wayland-0`. Defaults also
include `HOME=/root`, `LANG=C.UTF-8`, `GDK_BACKEND=wayland` and
`XMODIFIERS=@im=trames`; explicit `--setenv`
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

Keyboard input follows the active host XKB-v1 map. Native X11 imports its core
keyboard map through libxkbcommon-x11 and listens for keyboard/map/state/control
notifications; native Wayland relays the compositor's keymap FD and repeat info.
Depressed, latched and locked modifiers and the layout group stay distinct.
Physical evdev events do not generate compositor-side repeats: Wayland clients
use repeat-info, and original Xwayland uses its own server repeat policy.
Pointer coordinates, buttons and wheel retain the existing `InputEvent` API.
Focus loss clears held keys/buttons while retaining native lock/group state.
Native window management is described below.

The original `--display x11` fixed Vulkan display remains a separate mode. It
cannot be combined with `--desktop`; `--display-size` applies only to that fixed
display. The host frontend is selected independently of whether the guest
application uses Wayland or original Xwayland.

## HiDPI and multiple outputs

Native Wayland imports host output identities, logical positions, pixel modes,
physical dimensions, rotation and logical sizes. X11 imports RandR monitor
identities, rectangles and physical dimensions, and derives logical geometry.
Two monitors at the same density remain distinct `wl_output` objects. Removal sends surface leave
before global removal; reinsertion has a fresh protocol lifetime. Native output
mode opts into the additive Runtime `desktop_outputs`/`window_outputs` API; the
existing Apple `output_scale` API and `WindowSnapshot` layout remain available.
No CPU execution path, page geometry or guest instructions change.

Window content/configure sizes, input positions, popup geometry and IME carets
remain logical. Buffers and native X11 window dimensions use pixels. Native
Wayland imports per-surface fractional-scale hints, integer buffer-scale hints
and output enter/leave; it uses a viewport with buffer scale 1 for fractional
buffers. Integer-only hosts use the corresponding integer buffer scale. A fresh
host hint wins; after migration/hotplug without a fresh hint, current output
membership supplies the fallback. Rendering rounds positive physical dimensions
to the nearest integer, with ties upward. SHM and Vulkan composition share this
size conversion. The supported density range is 1x–4x (1/120 units).

X11 imports RandR 1.5 monitors and watches RandR notifications, root configuration
and resource changes. Its monitor rectangles are already oriented, so the guest
announcement uses an oriented pixel mode with normal transform and nominal
60 Hz. Native presentation still uses the host WSI cadence. Older servers fall
back to the root screen. `Xft.dpi` in
`RESOURCE_MANAGER` supplies the session density; otherwise physical DPI supplies
a quarter-step estimate. X11 has no universal per-monitor UI-density setting.
Override it by connector/monitor name when necessary:

```sh
TRAMES_DESKTOP_OUTPUT_SCALES='DP-1=1.25,HDMI-1=2' trames run --desktop x11 --rootfs ROOT -- /usr/bin/app
```

`*=1.5` supplies a default override. Values must be finite and within 1–4;
settings are read at desktop setup. Floating X11 windows preserve logical size
when crossing monitors; maximized/fullscreen sizes follow host confirmation.
Minimum/maximum size hints are converted to physical pixels. Explicit guest
fullscreen output selection maps to the native Wayland output, or to matching
Xinerama indices through EWMH on X11; the host window manager determines support.
X11 monitors without a matching Xinerama screen use normal WM placement.

Guest clients must use buffer scale or fractional-scale/viewporter to render
additional detail. Legacy X11/Xwayland applications can continue using their
existing resolution; increasing the presentation scale cannot create detail
absent from their buffers. There is no claim of automatic toolkit DPI repair.
The multi-output topology describes screens, not global placement of individual
native Wayland toplevels (which that protocol does not expose).

`python3 scripts/validate-hidpi.py --unit-binary UNIT --output OUT` exercises an
original Sway headless compositor and Xvfb/Openbox, including 125%/150%/2x,
migration, native pixel stripes, logical pointer coordinates, scaled size hints,
rotation, fullscreen target selection and hotplug. It requires Sway, grim and
xrandr in addition to the normal native test dependencies. The retained virtual
pointer XML carries its upstream MIT notice. Add `--runner RUNNER --receipt JSON`
for both actual KVM transports with one/four workers. These are software display
checks; physical GPU/multiple physical monitor qualification remains separate.

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

## Host clipboard and input methods

Both frontends attach the existing shared `HostClipboard` interface. Text imports
and exports use nonblocking pipes on the original foreground `HostWait`, with
bounded active transfers and finite transfer deadlines. The frontend adds no
clipboard service thread or polling timer. Host text follows the shared API's
16 KiB UTF-8 limit; guest-to-guest MIME negotiation retains its existing behavior.
X11 supports TARGETS/UTF8_STRING/TEXT, Latin-1 STRING fallback and incoming INCR.
Wayland uses the host seat data device and data sources; a private MIME marker
suppresses feedback when the frontend receives its own exported selection.

X11 uses the host's XIM server selected by its `XMODIFIERS` and locale. Native
Wayland uses text-input-v3, with text-input-v1 fallback for hosts such as Weston
13. Surrounding text, UTF-8 cursor positions, caret rectangles, preedit, commit
and surrounding deletion pass through the shared text interface. Context IDs,
epochs and protocol serials reject edits for an obsolete focus/composition.
The host input method owns candidates; guest Xwayland uses the shared Runtime
XIM server. If no host input method is available, ordinary keyboard input remains
available. A native Wayland compositor must expose a supported text-input
protocol for its input method to be bridged.

## Vulkan desktop composition

`--desktop-renderer auto` is the default. SHM-only windows use the software path;
GPU-backed windows use native Vulkan composition. `software` selects software
SHM composition explicitly and rejects GPU-only planes. `vulkan` also uploads
and composes SHM planes on Vulkan. Enabling a desktop alone does not initialize
a GPU for ordinary SHM windows. Install the original guest Vulkan ICD using the
existing `--vulkan-icd` option when running Vulkan applications:

```sh
trames run --rootfs /path/to/rootfs --desktop wayland \
  --vulkan-icd /path/to/libtrames_vulkan_icd.so \
  --setenv VK_DRIVER_FILES=/usr/share/vulkan/icd.d/trames.json -- /usr/bin/app
```

GPU planes retain their original native image/device/queue. The renderer samples
those images directly, without CPU pixel readback; only SHM planes upload.
Per-window pipelines/swapchains retain a device lease, separate from source image
slot leases. Source frames remain retained through the presenter's submission
fence. Queue entry points share the Runtime's actual per-queue synchronization;
there is no device-wide idle per frame. Current GPU planes within one scene must
share their native device and queue. Separate windows may use separate devices.
Cross-device/mixed-queue composition and general external DMA-BUF synchronization
are subsequent work.

The X11 WSI connection is separate from the input connection. Native requests
are ordered before an explicit XPresent MSC notification, including for Mesa
software paths that do not issue XPresent themselves. Wayland uses host frame
callbacks and retains a prepared read across the original HostWait, cancelling
it before WSI operations so the driver's libwayland queue cannot hide UI events
or deadlock a native roundtrip. Both paths pace frames through actual host events.
Renderers are destroyed before their native windows/surfaces and connections.

## Interoperability acceptance

`python3 scripts/validate-desktop-interop.py --native-only --unit-binary UNIT --output OUT`
runs eight native reference cells. Add `--runner RUNNER --guest-icd ICD` and omit
`--native-only` for 24 further real ARM64 KVM cells: both transports, one/four
vCPUs, X11 and native Wayland clipboard/IME, plus original guest Vulkan rendering.
`--case ID` reuses unaffected evidence by selecting an affected cell. Each case
uses an isolated scratch rootfs and retains command lines, logs and input hashes.
The source-bound executables are frozen before the matrix starts.

Original Weston provides the native Wayland host. Its desktop shell supplies the
real text-input/input-method-v1 path; its kiosk shell is used for deterministic
pixel capture. Original X11/XIM/libwayland/Vulkan protocol peers verify Unicode
text in both directions and preedit/deletion/commit. GPU cases render sixteen
frames over two swapchain generations, compare actual host pixels, then tear
down successfully. Hosted ARM64/AMD64 CI uses Mesa software Vulkan; these checks
make no physical GPU or performance claim. See the
[2026-11-26 record](../validation/desktop-interop-20261126.md).

The additional `scripts/validate-xwayland-ime.py` gate builds an original Xlib
client and runs four real KVM cells against original distro Xwayland. Supply
`--runner`, `--unit-binary`, a desktop rootfs from `validate-desktop.py`,
`--launcher` and `--output`. It copies only the test rootfs and the original
X11/C.UTF-8 locale data into per-case scratch trees. The client uses Xlib's
standard input-method instantiate callback if Xwayland starts accepting clients
before its XIM server is registered. This exercises confirmed Unicode input
through both the host XIM and the shared guest XIM service. Xlib development
headers are needed for this extra gate (`X11_CFLAGS` and `X11_LIBS` may select
an extracted development package).

## Host keyboard layouts

The X11 frontend requires the host's `libxkbcommon.so.0` and
`libxkbcommon-x11.so.0` (Ubuntu/Debian: `libxkbcommon-x11-0`). These are loaded only
when selecting host X11. Headless execution and the native Wayland frontend do
not load the X11 keyboard library. XKB extension negotiation and keymap/control
queries occur at setup or configuration changes, not for ordinary keys or CPU
execution. Detectable autorepeat is enabled on the frontend's own X11 client;
host settings are not changed.

The shared Runtime accepts an owned, NUL-terminated XKB-v1 map and creates its
retained FD before enqueueing the update, so allocation/I/O failures reach the
frontend. A map replacement updates existing keyboards and republishes the
current modifier state. Newly bound clients receive the current map, repeat
policy and state. Existing Apple callers retain the original Key event API and
its default US map; no Linux API, dependency or ABI check enters CPU execution.

Xwayland key events and input-method commits retain the existing XWM connection.
Physical events target Xwayland's enumerated native keyboard using the public
XInput event form of XTEST. Core XTEST's separate default-US device would otherwise
replace the master map on first input. Native modifier/lock/latch/group updates
also use that XWM connection, avoiding a second Wayland state update toggling
Caps Lock again. The pinned XKB request binding omits the modLatches wire byte;
the adapter fills that public field, checked against the original distribution
C header at compile time in `tests/fixtures/keyboard-control.c`.

`python3 scripts/validate-keyboard.py --native-only --unit-binary UNIT --output OUT`
compares the original X11/Wayland references and production frontends. Add
`--runner RUNNER --receipt RECEIPT --rootfs DESKTOP_ROOTFS --launcher LAUNCHER`
to run the 16-cell real KVM matrix for both transports, one/four vCPUs, both host
frontends and guest Wayland/Xwayland. The gate uses original Xlib/libxkbcommon,
actual German/French/Russian layouts, AltGr, Caps Lock across a map change,
Num Lock, layout groups, focus release and distinct physical/server repeat
semantics. Xlib development headers, `setxkbmap` and `xset` are test prerequisites.
No coordinator or input service thread is added. See the
[keyboard validation record](../validation/keyboard-20261126.md).

## Native window management

Both native frontends opt into host-managed window confirmation. Guest
maximized, fullscreen and minimized requests remain independent. The existing
Apple three-mode embedding API and `WindowSnapshot` layout remain available;
Linux uses additive state/request maps in the shared server snapshot. Per-field
request generations retain the order of distinct operations and prevent an
older native reply from erasing a newer, undispatched guest request. Repeating
a request still reaches the host; unrelated state operations are not invented.

X11 uses ICCCM `WM_STATE`/`WM_CHANGE_STATE` and EWMH `_NET_WM_STATE`, including
original Xwayland clients. Native Wayland consumes the toplevel state and size
together at the following `xdg_surface.configure` acknowledgement. The frontend
commits the corresponding native frame before confirming it to the guest and
forwarding later state requests. Native frame readiness drives this ordering.
The host confirms actual geometry. Keyboard focus follows native
`wl_keyboard.enter/leave`; xdg `activated` describes window decorations and
must not restore keyboard focus after a native leave. Maximized and fullscreen configurations
bypass ordinary floating size constraints. A zero-sized restore suggestion
retains the last confirmed floating content size. CSD extents belong to the
native surface/framebuffer; xdg window geometry and guest configure sizes
refer to content. Every native commit after an acknowledgement already uses
that confirmed geometry, even if Xwayland's new buffer has not arrived yet.
Parent/transient metadata and min/max constraints update
without requiring a new guest frame; parent cycles are rejected. X11 reads
current native geometry for state replies and suppresses stale guest resize
responses while waiting for the latest host size; genuine application-driven
resizes remain supported.

Interactive move/resize uses a real native button press, not the guest's
protocol serial. X11 sends `_NET_WM_MOVERESIZE` with native root coordinates;
Wayland uses the host seat and native press serial. When the host Wayland
compositor takes the grab, the frontend releases the guest's implicit grab,
because the host consumes the eventual physical release. Existing foreground
waits and compositor commands carry these operations; no service thread or
polling timer is added.

`python3 scripts/validate-windows.py --native-only --unit-binary UNIT --output OUT`
compares original Xlib/libwayland peers on Openbox and Weston with the production
frontends. Add `--runner RUNNER --receipt RECEIPT --rootfs DESKTOP_ROOTFS
--launcher LAUNCHER` for SMCCC/MMIO × one/four vCPUs × host X11/Wayland × guest
Wayland/Xwayland. The gate measures actual host geometry/pixels and uses real
host input for move, resize, minimize/task-switch restore and parent focus.
Openbox, Weston, Xvfb and the original X11/Wayland development packages are test
prerequisites. This gate does not qualify physical GPU acceleration, fractional
scaling, multiple outputs or activation-token protocols.
