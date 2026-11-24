# Linux PCM provider

The shared `trames-audio` profile `0x10008` keeps its existing 128-byte records
and guest ALSA ioplug. Original guest libraries and instructions are unchanged.
Linux selects a native libasound provider at compile time; Apple keeps CoreAudio.

The Linux host loads `libasound.so.2` only on an authorized PCM open. It requires
no development headers or mandatory audio linkage for a normal product build.
`--audio` grants playback and `--audio-input` grants capture to foreground
run/exec/shell. Both default off. The independent shared CLI library stays pure;
these product flags are translated to existing `RuntimeOptions` by Linux.

`TRAMES_AUDIO_OUTPUT_DEVICE` and `TRAMES_AUDIO_INPUT_DEVICE` name host ALSA PCMs,
with `default` as their default. These are host environment variables, separate
from guest `--setenv`. PipeWire/PulseAudio can be reached through an installed
ALSA PCM plugin; there is no direct PipeWire provider in this batch. A missing
library or incompatible symbol returns ENOTSUP; device-open/configuration errors
retain ALSA's errno. Headless sessions never initialize an audio library/device.

## Transfer and timing

Native PCM opens nonblocking with interleaved S16_LE or FLOAT_LE, one/two channels
and the existing profile's 8–192 kHz range. Requested rate, period and buffer must
be accepted exactly, including the final negotiated parameters. The provider
returns EINVAL if the host cannot match them. Guest ALSA's plug layer can perform
conversion/mixing; the runtime does not silently change the guest ring geometry.
The software start threshold prevents implicit start before the guest starts.

Short controls, readi/writei and status run on the calling guest worker under its
original per-stream mutex. There is no new Trames service/coordinator thread,
periodic readiness timer or runtime sample queue. The existing 64 KiB scratch
bounds transfers. ALSA/plugin-owned threads, buffers and conversion costs are
properties of the selected host PCM. This is API passthrough, not zero-copy PCM.

The v1 status retains position, queued/capture frames, availability, XRUN counts,
rate, channels and activity. Position uses native delay/availability and the
number of frames transferred, and stays monotonic until prepare. Native errors
including EAGAIN, EPIPE, ESTRPIPE and ENODEV propagate. A native poll error is
latched until prepare so a waiter cannot wake into a falsely successful status.
XRUNs are counted once until prepare. The v1 record has no hardware timestamps;
clock calibration, device migration and A/V synchronization remain release work.

## Readiness and lifetime

The session retains the original guest eventfd object before reading the rest
of an open record, preserving FD-before-record fault ordering. The notification
endpoint registers weak native producers on that counter; it never resolves a
later reused descriptor number. Ordinary eventfd reads/writes keep their existing
atomic implementation. Callback-only producers do not allocate a producer list.

When a consumer actually waits, its existing `HostEvents` queue registers owned
duplicates of the ALSA poll descriptors. Their POLLIN/POLLOUT directions come
from ALSA, including playback plugins that report readable timer/event FDs.
The idle executor invokes ALSA poll_descriptors_revents to filter/consume native
notifications, then signals the original guest eventfd only for PCM readiness or
an error. A filtered intermediate event rearms the one-shot subscriptions. No
extra polling thread or periodic fallback is introduced.

A weak source points to the same outer stream mutex used for controls/transfers.
Controls cancel old descriptor registrations before potential descriptor changes,
then rebuild them for existing waiters. Close unregisters duplicates before
native close. Late batches retain only a weak source. Multiple streams may share
one original eventfd; closing one cannot remove another producer. No strong
cycle joins the stream, eventfd, source and host queue.

## Compile-time ABI checks and validation

`runtime/scripts/check-alsa-abi.py` translates the actual Rust FFI declarations
into C type assertions and compiles them against the installed ALSA headers.
It also asserts the constants used by the adapter. CI installs `libasound2-dev`
for this check; no check enters the product execution path. Platform selection
and the existing guest record layout remain compile-time contracts.

Runtime tests use libasound's null/file devices for exact sample checks and a
test-only ALSA ioplug with real kernel timerfd readiness. It intentionally filters
its first event and injects XRUN/suspend/removal errors. Tests cover capture bytes,
partial/nonblocking transfer, controls, repeated close/prepare, shared notification
objects, guest FD close/reuse and native registration teardown. These establish
software API/error plumbing, not physical sound-device acceptance.

`scripts/validate-audio.py` compiles the unchanged guest terminal plugin and one
ALSA client. The same client runs natively and through SMCCC/MMIO KVM, comparing
samples/output and checking disabled-by-default capabilities. Real sound hardware,
XRUN recovery under load, unplug/replug, latency and A/V clock behavior still
require a media-capable Linux machine.

The native API contract is documented by [ALSA PCM](https://www.alsa-project.org/alsa-doc/alsa-lib/group___p_c_m.html)
and [PCM status](https://www.alsa-project.org/alsa-doc/alsa-lib/group___p_c_m___status.html).
