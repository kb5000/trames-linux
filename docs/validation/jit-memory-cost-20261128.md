# JIT memory cost and private-page admission — 2026-11-28

The measured gap comes primarily from **per-access software handling around a
native LDR/STR**, rather than the final memory opcode. In a resident-page diagnostic
build, the tiny native memory fragment accounts for about **1.3% of guarded user
CPU samples**, or **1.7% after existing exclusive-private admission**. The rest is
mostly software address/page checks, generic byte borrowing, helper work and
metadata, plus the generated block's calls/register traffic.

## Source-confirmed access path

A normal generated memory operation currently:

1. Flushes dirty guest register bindings; saves native context/count/budget/LR
   on a balanced 48-byte host stack frame and calls a typed Rust helper.
2. Reads predecoded address metadata and guest registers, computes the address,
   checks page crossing and enters `CpuMemory::cpu_read_chunk/cpu_store_chunk`.
3. Uses View translation/permission checks, confirms page readiness, constructs
   the existing byte-region guard and validates its slice range. Shared access
   uses the original regional atomics; eligible exclusive ownership bypasses them.
4. Indirectly calls the three-word BTI/LDR-or-STR/RET native fragment on those
   checked bytes. Stores retain dirty/exclusive/code publication.
5. Commits results/base writeback, updates native-memory counters, checks code
   identity, refreshes the register sub-borrow and returns to the generated block.
   Later ALU operations reload any bindings flushed at the memory boundary.

The last fusion removed main-dispatch returns between operations, but these
per-memory helper boundaries remain. Known width/address kinds still flow through
a general decoded-operation handler; it is not yet an inline native memory fast
path. Successful access therefore pays for considerably more than one load/store.

KVM prepares mappings and permissions in its page tables and executes ordinary
EL0 loads/stores through the hardware TLB/MMU/cache. A warm ordinary access does
not call Rust or produce a user-space KVM exit; missing/protected mappings and
other exceptions take the exit path. The load/add/store values stay in hardware
registers between those instructions. This is the key structural difference.

## Software-clock sampling

Diagnostic copies only; production code, dependencies and pins are unchanged.
Linux source is `fe7044e`, common Runtime `89c5abd`. The payload is the same finite
1,048,576-iteration LDR/add/STR/SUBS/B.NE loop with one final SVC. The data page is
explicitly materialized before creating the View in both modes. One actor owns
all Memory handles and the sole View; no clone, mapping mutation, fork or external
byte actor runs during the epoch. Existing `View::try_private_epoch` is used;
both actual admission **and target-page eligibility** are checked before timing.

The only diagnostic Runtime additions expose that eligibility before execution
and register cold RX addresses in `/tmp/perf-PID.map`. There is no per-access
profiler hook. CPU helper bodies and ordinary byte operations are unchanged.
The benchmark copy selects this one memory payload and exposes guarded/private
modes. [The exact compressed patch](jit-memory-cost-20261128/diagnostic.patch.gz)
and [source/binary receipt](jit-memory-cost-20261128/receipt.json) bind the copy.

`perf cpu-clock:u`, 997 Hz, samples 24 fresh non-root benchmark processes per mode,
including each process's one warm-up and five timed passes. Sampling is separate
from latency controls. It records **4,645 / 4,343 samples**, with zero lost samples.
No callgraph is claimed: these are leaf/self-IP shares, grouped by symbol name
across per-process anonymous maps. Inlined operations are attributed to their
containing symbol; percentages are not exact independently removable cycle costs.
This VM exposes software events, not a usable hardware PMU cycle breakdown.

| Sampled symbol group | Guarded | Exclusive-private |
| --- | ---: | ---: |
| Address/page checks (`reached`, `load`, `ensure_resident`, zero-read note) | 25.9% | 28.6% |
| Decoded access / typed helper (`Operation::execute`, `memory_access`) | 22.5% | 23.1% |
| Byte callbacks / range proof (`cpu_*_chunk`, `Guard::covers/write`) | 23.1% | 26.8% |
| Regional guard creation / outlined atomic primitives | 16.8% | 8.0% |
| Dirty/monitor metadata (`changed`, `invalidate`) | 5.2% | 5.7% |
| Generated block and native memory leaf | 6.4% | 7.9% |

The final row includes ALU/branches, generated call scaffolding and the memory
leaf; the leaf itself is **1.29% / 1.68%**. There is only one actor, so the guarded
row describes uncontended protection costs, not measured lock contention. Outlined
atomic primitives may serve more than one metadata operation; their shares are
not all assigned to one lock. Page-readiness symbols mainly check an already
resident page here, not a stream of real page faults. All payloads have zero
interpreter fallback, zero staging and the correct canonical result 6,291,456.

## Admission control, and correction to the preceding comparison

The prior report's `worker` label meant the retained `Cpu::run_parallel` View API.
It **did not invoke the session scheduler's private-page admission**, and its
anonymous data page was first written by the guest. Calling that the general
production-worker performance was too broad. The Runtime already admits safe
private epochs in process dispatch; single-actor eligible frames can skip regional
atomics. It also has a fixed-width MMU fast path used by the interpreter.

Separate unprofiled controls alternate guarded/private order six times per
software engine, using the **same explicitly resident eligible page** in both.
Each process has one warm-up and five timed samples. Median of six process medians:

| Resident data page, 1,048,576 iterations | Guarded | Existing private admission |
| --- | ---: | ---: |
| JIT | 30.939 ms | 28.752 ms |
| Interpreter | 24.014 ms | 21.805 ms |

JIT time falls about **7.1%**, not enough to remove the main software-path cost.
Even `unlocked` guard construction/range proof remains visible. On this eligible
resident/private setup the interpreter is faster than JIT; the earlier guarded,
demand-populated result does not establish a universal JIT/interpreter ranking.
These controls have different setup/diagnostic code from the preceding three-way
cohort; no old KVM number is spliced into them or assigned a new private-mode ratio.

Private admission alone is not a complete optimization. The next useful work is
to lower common resident-page address/permission fast paths into generated code,
reuse the existing MMU/TLB interface, and keep guest values in registers across
normal accesses. Slow exits must still handle missing/protected/COW/cross-page
and executable writes with precise state. Each access retains the original guest
memory/concurrency contract; no widened LDR/STR RMW lock is introduced. Dynamic
addresses and permissions still need runtime or hardware validation; instruction
kind/width/address form are translation-time facts.

## Evidence and reproduction

[All aggregated symbols](jit-memory-cost-20261128/sample-summary.json), complete
compressed perf reports/data, the 48 symbol maps, all 48 sampled-process results,
24 unprofiled control records and their summary are [retained together](jit-memory-cost-20261128/).
The receipt pins source hashes, diagnostic binary, scope and patch. Apply the
patch to the isolated source preparation described in the preceding benchmark
README, build release with its locked toolchain, run `jit-cost-pairs.py` separately
from perf, then sample `jit-cost-profile-loop.sh worker` / `worker-private`.
Perf needs permission for the user software event; this run elevated only perf,
ran the child workloads as the existing user and changed no persistent perf policy.

Native RX maps must be retained until symbolization. The script/report command
sorts symbols, and repeated anonymous names are summed across their process maps.
Original production throughput numbers are not rerun or replaced by sampled runs.
No SDK/NDK/JDK, emulator, rootfs or additional VM was installed.

The [offline audit](jit-memory-cost-20261128/audit.py) recomputes named sample
aggregation, checks both actual private flags and eligible target pages, validates
all exact payload results/control medians, verifies 48 symbol maps and binds the
compressed patch hash. Its [result](jit-memory-cost-20261128/audit-result.json)
requires no VM or workload. Re-symbolizing raw perf data additionally needs the
matching ELF/build-id; frozen symbolized reports and maps are supplied, while
temporary executables are deliberately not archived.
