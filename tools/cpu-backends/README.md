# Three-backend CPU comparison

This isolated AArch64 Linux package compares current Trames interpreter/JIT/KVM
execution on identical finite guest instruction bytes. It is outside the product
workspace and introduces no product or iPad JIT dependency. Architecture/JIT
requirements are checked at compile time. `/dev/kvm`, real 4 KiB pages and actual
SMCCC forwarding are required for the KVM lane; unsupported hardware fails.

The primary `worker` mode uses the same `Cpu::run_parallel` interface with a
retained canonical Memory/View for all three engines. Backend-specific hardware
admission occurs before warm-up. Normal KVM 2 ms assignments and short-SVC budget
reuse stay enabled. `direct` is a separately labelled optional coordinated API
measurement; its timings must not be mixed with worker results.

Compute payloads have finite counters and one terminal original SVC. The KVM
backend does not expose exact retirement counts; matching loop counters, x0,
final PC, canonical memory, and unchanged original code establish equal work.
Software observed counts must match the known payload count. Short SVC loops
include an ordinary backbranch and simulated x0 return, excluding real Linux
syscall handlers. Native compute/hardware instructions and KVM execution are not
the same thing as native host Linux applications or a bare-metal KVM baseline.

Each process runs one warm-up and five timed samples per workload. The controller
uses all six backend order permutations, so each backend occupies each position
twice. It retains every sample and verifies matched instruction bytes/work and
results before computing the median of six process medians. Host memory bytes
are validated after all samples, avoiding a fresh KVM alias revocation between
timed passes. Compilation, VM creation and initial page faults are warmed rather
than charged to every sample.

## Source pins and reproduction

The 2026-11-28 cohort uses Linux transport source
`9db947eba448b871f31ca06511c46430bfb35387` and common Runtime
`89c5abd57ce5cc8341f17fcea35a339b0f991595`. The Linux product pin remains
unchanged. **Use an isolated source directory with this Runtime override**;
the product's preceding pin does not yet contain this JIT. Every compiled source
hash, the isolated lockfile and the release binary hash are retained in receipts.

From a Linux checkout containing this measurement tool and a local clone of the
shared Runtime containing the specified commit:

```sh
# Set these to existing local clones. No SDK, rootfs or new VM is needed.
TRAMES_LINUX_REPO=$PWD
TRAMES_RUNTIME_REPO=/path/to/trames-runtime
TRAMES_MEASURE_DIR=/tmp/trames-cpu-comparison
mkdir -p "$TRAMES_MEASURE_DIR/source/runtime" "$TRAMES_MEASURE_DIR/source/tools"
git archive 9db947eba448b871f31ca06511c46430bfb35387 Cargo.toml Cargo.lock rust-toolchain.toml crates | tar -x -C "$TRAMES_MEASURE_DIR/source"
git -C "$TRAMES_RUNTIME_REPO" archive 89c5abd57ce5cc8341f17fcea35a339b0f991595 | tar -x -C "$TRAMES_MEASURE_DIR/source/runtime"
cp -R tools/cpu-backends "$TRAMES_MEASURE_DIR/source/tools/"
export CARGO_TARGET_DIR="$TRAMES_MEASURE_DIR/target" CARGO_INCREMENTAL=0
cd "$TRAMES_MEASURE_DIR/source/tools/cpu-backends"
cargo build --release --locked
TRAMES_KVM_TRANSPORT=smccc taskset -c 2 "$CARGO_TARGET_DIR/release/trames-cpu-backends" kvm worker --quick
python3 "$TRAMES_LINUX_REPO/scripts/benchmark-cpu-backends.py" \
  --binary "$CARGO_TARGET_DIR/release/trames-cpu-backends" \
  --source "$TRAMES_MEASURE_DIR/source" --output "$TRAMES_MEASURE_DIR/results" \
  --linux-revision 9db947eba448b871f31ca06511c46430bfb35387 \
  --runtime-revision 89c5abd57ce5cc8341f17fcea35a339b0f991595
```

The controller refuses to overwrite an existing cohort. Reuse an existing target
cache when storage is tight. Source snapshots, temporary executables and any new
build cache may be removed after preserving the receipt and outputs. Physical
host CPU placement and bare-metal KVM measurements remain separate from nested
VM results.
