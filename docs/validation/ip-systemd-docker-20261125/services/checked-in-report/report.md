# Runtime application acceptance

Each cell is one workflow in one execution configuration. Missing/blocked/reference failures do not count as passes.

| Workflow | native | kvm-smccc-1 | kvm-smccc-4 | kvm-mmio-1 | kvm-mmio-4 |
| --- | --- | --- | --- | --- | --- |
| containerd.shim-startup | passed | passed | passed | passed | passed |
| docker.lifecycle | passed (reused) | passed (reused) | passed (reused) | passed | passed |
| systemd.pid1-services | passed (reused) | passed (reused) | passed (reused) | passed (reused) | passed |

Timing includes setup/startup and is not a qualified performance comparison.
