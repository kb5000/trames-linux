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

The installed Weston 13 reference did not return the expected error for this
request sequence. That observation is not counted as a qualified negative case.
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

## Cloud closure

Product implementation `fcababc` passes
[ARM64 Linux, AMD64 Linux and macOS build/unit/release checks](https://github.com/project-trames/trames-linux/actions/runs/45886029729),
[ARM64/AMD64 native presentation](https://github.com/project-trames/trames-linux/actions/runs/45886029629)
and [notices](https://github.com/project-trames/trames-linux/actions/runs/45886029623).
Both new parent-error frontend cells pass on each native architecture. Exact
statuses and log lines are archived in
[cloud-checks.json](window-parent-20261127/cloud-checks.json).

The concurrent shared Runtime update left its generated Cargo.lock fingerprint
stale. Main `087059d` refreshes only `third-party/inputs.json`; routing commit
`e4632eb` excludes that noncompiled fingerprint file from Runtime CPU checks.
Actual dependency manifests still trigger the full pipeline. The
[notice-only verification](https://github.com/project-trames/trames-runtime/actions/runs/45888352646)
passes with 171 reviewed packages and 109 original notice texts, and the API
records confirm that this metadata push ran only the notice workflow. Linux's separate
139-package product inventory and tested Runtime pin remain
attached to their original source cohorts.

The [follow-up scratch cleanup](window-parent-20261127/scratch-cleanup.json)
removes redundant red-check binaries and retains both accepted frozen pairs.
The combined compact reports contain source/command/log evidence, without VM
images, rootfs trees or executable archives in Git. The current source audit
and the preserved 89-cell archive audit both pass.

