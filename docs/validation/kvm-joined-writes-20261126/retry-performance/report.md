# Paired original workload baseline

Audit: **passed**.
5 measured executions; calibration/warmup excluded.

Medians are milliseconds. Ratio is KVM/native work time (lower is better).
Wall and CPU include startup; sampled memory may miss short peaks.
This host/configuration only; caches retained; no bare-metal claim.

| Workload | Workers | Lane | Work ms | Wall ms | CPU ms | Work/native |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| build | 1 | baseline-mmio | 25439.443 | 25517.464 | 25037.948 | 8.061 |
| build | 1 | baseline-smccc | 25586.969 | 25667.858 | 25243.909 | 8.108 |
| build | 1 | candidate-mmio | 25433.863 | 25514.368 | 25065.509 | 8.059 |
| build | 1 | candidate-smccc | 22653.999 | 22734.131 | 22368.859 | 7.179 |
| build | 1 | native | 3155.780 | 3156.856 | 3124.218 | 1.000 |

## Candidate / baseline time ratios

| Workload | Workers | Transport | Work ratio | Wall ratio |
| --- | ---: | --- | ---: | ---: |
| build | 1 | mmio | 1.000 | 1.000 |
| build | 1 | smccc | 0.885 | 0.886 |
