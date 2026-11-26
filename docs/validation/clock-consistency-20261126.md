# VM wall-clock and make consistency — 2026-11-26

The make warning from the [joined-writes comparison](kvm-joined-writes-20261126.md)
was caused by the nested-KVM test VM's wall clock stepping backwards. Native
Linux reproduces the same failure. Runtime's realtime/file-timestamp code and
application instructions are unchanged.

## Diagnosis and repair

The Lima 2.2.1 agent was periodically setting the VM clock while the VM's
systemd-timesyncd was also active and synchronized. A 25-second original static
ELF detects **three backward steps in each of native, SMCCC and MMIO**, with a
maximum lower bound of **153.773 ms** and three future inode timestamps per
lane (maximum **150.045 ms**). The interval method brackets each realtime read
with monotonic reads, so ordinary call preemption cannot explain these steps.
Agent SyncTime logs coincide with this diagnostic. The original make warning,
exit-zero result and correct checksum remain in the evidence.

Lima's pinned source sends the host timestamp every ten seconds and calls
settimeofday when measured drift exceeds 100 ms. This explains why competing
clock owners can leave recently written files in the future. The evidence does
not isolate how much of the measured drift came from host/NTP disagreement or
RPC latency; no claim about that split is needed for the repair.

The VM now has a persistent drop-in at
`/etc/systemd/system/lima-guestagent.service.d/90-trames-clock.conf`:

```ini
[Service]
CapabilityBoundingSet=~CAP_SYS_TIME
```

[configure-lima-clock.sh](../../scripts/configure-lima-clock.sh) requires active,
synchronized systemd-timesyncd before applying the drop-in and restarting only
the Lima agent. Effective, permitted and bounding capabilities confirm that
CAP_SYS_TIME is absent. A repeated apply preserves the agent PID. NTP, SSH and
both existing source/output mounts remain available. Denied Lima SyncTime calls
remain visible in the journal; warnings are not filtered. The VM was not rebooted
for this change. The drop-in is persistent and was applied through a service
restart; a full VM reboot/suspend-resume is outside this result.

## Scoped validation

| Check | Result |
| --- | --- |
| Before repair, native/SMCCC/MMIO, 25 s | 3/3 detect backward steps and future inode times |
| After repair, byte-identical ELF, native plus SMCCC/MMIO at 1/4 CPUs, 35 s | 5/5 pass |
| Committed clock gate, same five lanes, 45 s | 5/5 pass; 81,497 samples, zero steps/future times above 1 ms |
| Original make/GCC, native plus old/new KVM with both transports, 1/4 CPUs | 10/10 pass; stderr empty; all 12 object hashes and checksums match native |
| Offline audit negative controls | Changed transcript, missing lane and rehashed future timestamp all rejected |

The clock gate is now required before the performance workflow collects its
original workloads. Its observer and source/receipt/log auditing are acceptance
tools, adding no product runtime checks or service threads. A finite 45-second
check is not a promise that CLOCK_REALTIME is monotonic. NTP startup, suspend or
an administrator may legitimately step it; rerun the gate after such events.

The old baseline is Linux `88b6d8505ad51a5a0d96fb10ba4e6666367eea0c`, runner SHA-256
`d9641f0fbd8af8b2ef04b4e854efebbdbee91329af0e0b8be49a10f2895d5378`.
The candidate is `c0955881e5554c659f8e15dc1c1c4cf129f02607`, SHA-256
`345289fd15f4fad74ccc971ccfcefbd725919c12f049e7c3c914c06ee77590e4`.
Both retain Runtime `a37a84d4b3c66f21ddb581f8f4b0427f50793e0d` and shared CLI
`da18836903d8e37bbb92cc32cc7ec78f8cf457bc`. Original immutable GCC/make/rootfs files
are reused and checked after testing. The tiny new clock fixture is separate.
The first build controller stopped after native execution because its result
record repeated a Python keyword; that source and raw output are retained as an
incomplete attempt, and only the corrected ten-case run qualifies.

[Raw evidence](clock-consistency-20261126/raw-evidence.tar.gz),
[index](clock-consistency-20261126/evidence-index.json),
[offline audit](clock-consistency-20261126/audit.py) and
[audit result](clock-consistency-20261126/audit-result.json) bind the native
failure, same-ELF comparison, exact clock/build commands, receipts, source hashes,
agent policy and original warning. Run the audit from a checkout with Runtime:

```sh
python3 docs/validation/clock-consistency-20261126/audit.py
```

The GCC/make rerun is a correctness check, not a replacement performance study.
The raw archive is about 1.3 MB. Only this task's temporary rootfs directories
were removed after export and hash checks; the 169-file immutable master remains
unchanged. Hard-linked tool bytes are shared, not counted as reclaimed storage.
Actual VM free-space recovery was 2.58 MiB.

Previous CPU, memory, network, media, Linux and Apple build results retain their
original scope. No Runtime/CLI pin, application, CPU backend or Apple source
changes, new VM/image or package download are part of this fix.

Primary sources: [Lima v2.2.1 timer](https://github.com/lima-vm/lima/blob/v2.2.1/pkg/hostagent/timesync.go),
[guest threshold/handler](https://github.com/lima-vm/lima/blob/v2.2.1/pkg/guestagent/api/server/server.go#L111-L133),
[Linux time setter](https://github.com/lima-vm/lima/blob/v2.2.1/pkg/guestagent/timesync/timesync_linux.go),
and [systemd capability bounding](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html#CapabilityBoundingSet=).
