# Runtime application acceptance

Each cell is one workflow in one execution configuration. Missing/blocked/reference failures do not count as passes.

| Workflow | native | kvm-smccc-1 | kvm-smccc-4 | kvm-mmio-1 | kvm-mmio-4 |
| --- | --- | --- | --- | --- | --- |
| containerd.shim-startup | passed | failed | timed_out | passed | timed_out |
| docker.lifecycle | passed (reused) | passed | passed | failed | passed |
| systemd.pid1-services | passed (reused) | passed | passed | passed | passed |

Timing includes setup/startup and is not a qualified performance comparison.
