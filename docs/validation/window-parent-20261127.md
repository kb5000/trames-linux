# Cyclic-parent protocol error correction — 2026-11-27

The current Linux pin is Runtime
[`7b4eae6`](https://github.com/project-trames/trames-runtime/commit/7b4eae68b12a480460b2ca71145826b59f7e4dfb),
with runner SHA-256
`63dbda755349387a80fca77ef4f2277b3ba84da6dfe6b96dc46bbabbcb203b24`.
This follow-up replaces the handwritten cyclic-parent error `3` with the
generated `xdg_toplevel::Error::InvalidParent` enum, whose standard wire value
is `1`. It changes only that rejected-request branch. See the
[exact patch](window-parent-20261127/runtime.patch) and
[build receipt](window-parent-20261127/build.json).

All **6** new negative cases pass: native X11/Wayland frontends plus real
ARM64 KVM SMCCC/one-vCPU and MMIO/four-vCPU on both host frontends. Each original
libwayland peer maps parent and child, requests a cycle and checks both the
protocol error's interface and its value. The independent scanner-generated
C header supplies the error enum and a compile-time wire-value assertion.
The [matrix](window-parent-20261127/matrix/results.json) also reproduces the
incorrect error `3` on the preserved old unit binary, which fails as expected.
No core dumps are produced.

The installed Weston 13 reference accepted this invalid cycle instead of
reporting the normative error. That observation is not counted as a pass.
The contract comes from the original header and the
[xdg-shell protocol definition](https://raw.githubusercontent.com/wayland-mirror/wayland-protocols/main/stable/xdg-shell/xdg-shell.xml),
rather than reproducing that reference behavior.

The existing [89-cell normal-flow cohort](windows-20261127.md) remains attached
to product `ed05b1b` and Runtime `aadee76`; it is reused without relabeling its
binaries or source. The [follow-up audit](window-parent-20261127/audit.py)
verifies that the pinned Runtime differs from that baseline only by this
error-expression replacement, checks every new protocol result and validates
the current checkout against its own exact-source receipt. Shared Wayland
regressions remain **33/33**; strict shared Linux/Apple checks pass.

Runtime main concurrently received the separately validated cgroup/resource-I/O
commit `8bfc094`. The tested protocol commit is retained unchanged and is
reachable through merge
[`acf1253`](https://github.com/project-trames/trames-runtime/commit/acf1253410eed9eb4deac9a3e223013a2f615ecf).
Linux pins the tested `7b4eae6` commit. This preserves both published histories
and keeps the two acceptance cohorts attached to their actual source.

```sh
python3 scripts/validate-window-parent.py --unit-binary UNIT --runner TRAMES \
  --receipt BUILD_RECEIPT --output NEW_OUT
python3 docs/validation/window-parent-20261127/audit.py --source-root "$PWD"
```

`--native-only` runs the two native frontend cases for ARM64/AMD64 CI.
`--baseline-unit OLD_UNIT` optionally reproduces the original error first.
The existing VM and build cache are reused; no VM image is created. These
checks do not qualify physical GPU acceleration or an AMD64 KVM backend.
Product cloud statuses are recorded after the paired pin is published.
