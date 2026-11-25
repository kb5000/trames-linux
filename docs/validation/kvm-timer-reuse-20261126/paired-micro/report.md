# Paired original workload baseline

Audit: **passed**.
150 measured executions; calibration/warmup excluded.

Medians are milliseconds. Ratio is KVM/native work time (lower is better).
Wall and CPU include startup; sampled memory may miss short peaks.
This host/configuration only; caches retained; no bare-metal claim.

| Workload | Workers | Lane | Work ms | Wall ms | CPU ms | Work/native |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| cpu | 1 | baseline-mmio | 68.127 | 101.369 | 98.093 | 0.971 |
| cpu | 1 | baseline-smccc | 74.206 | 106.399 | 103.588 | 1.058 |
| cpu | 1 | candidate-mmio | 68.309 | 99.702 | 95.249 | 0.974 |
| cpu | 1 | candidate-smccc | 67.884 | 97.969 | 95.389 | 0.968 |
| cpu | 1 | native | 70.126 | 71.468 | 70.422 | 1.000 |
| cpu | 4 | baseline-mmio | 128.732 | 191.833 | 346.977 | 1.310 |
| cpu | 4 | baseline-smccc | 110.165 | 166.113 | 343.855 | 1.121 |
| cpu | 4 | candidate-mmio | 107.394 | 165.082 | 353.485 | 1.093 |
| cpu | 4 | candidate-smccc | 109.850 | 163.174 | 347.585 | 1.118 |
| cpu | 4 | native | 98.283 | 100.389 | 268.373 | 1.000 |
| getpid | 1 | baseline-mmio | 608.459 | 631.977 | 626.707 | 445.304 |
| getpid | 1 | baseline-smccc | 491.869 | 513.490 | 509.933 | 359.978 |
| getpid | 1 | candidate-mmio | 592.671 | 615.760 | 607.406 | 433.750 |
| getpid | 1 | candidate-smccc | 459.782 | 480.702 | 473.475 | 336.494 |
| getpid | 1 | native | 1.366 | 2.288 | 1.990 | 1.000 |
| getpid | 4 | baseline-mmio | 1032.733 | 1090.649 | 3069.476 | 384.083 |
| getpid | 4 | baseline-smccc | 795.763 | 840.669 | 2324.707 | 295.952 |
| getpid | 4 | candidate-mmio | 1016.529 | 1071.396 | 3016.101 | 378.057 |
| getpid | 4 | candidate-smccc | 768.252 | 815.570 | 2292.390 | 285.720 |
| getpid | 4 | native | 2.689 | 4.268 | 6.393 | 1.000 |
| idle | 1 | baseline-mmio | 1005.442 | 1027.534 | 21.997 | 1.004 |
| idle | 1 | baseline-smccc | 1003.816 | 1024.178 | 20.194 | 1.002 |
| idle | 1 | candidate-mmio | 1005.526 | 1029.252 | 21.036 | 1.004 |
| idle | 1 | candidate-smccc | 1008.348 | 1025.642 | 20.106 | 1.007 |
| idle | 1 | native | 1001.603 | 1002.441 | 0.715 | 1.000 |
| idle | 4 | baseline-mmio | 1010.392 | 1053.502 | 37.991 | 1.004 |
| idle | 4 | baseline-smccc | 1013.610 | 1054.883 | 36.200 | 1.007 |
| idle | 4 | candidate-mmio | 1009.978 | 1055.702 | 38.764 | 1.004 |
| idle | 4 | candidate-smccc | 1013.542 | 1049.506 | 34.533 | 1.007 |
| idle | 4 | native | 1006.394 | 1007.466 | 1.230 | 1.000 |

## Candidate / baseline time ratios

| Workload | Workers | Transport | Work ratio | Wall ratio |
| --- | ---: | --- | ---: | ---: |
| cpu | 1 | mmio | 1.003 | 0.984 |
| cpu | 1 | smccc | 0.915 | 0.921 |
| cpu | 4 | mmio | 0.834 | 0.861 |
| cpu | 4 | smccc | 0.997 | 0.982 |
| getpid | 1 | mmio | 0.974 | 0.974 |
| getpid | 1 | smccc | 0.935 | 0.936 |
| getpid | 4 | mmio | 0.984 | 0.982 |
| getpid | 4 | smccc | 0.965 | 0.970 |
| idle | 1 | mmio | 1.000 | 1.002 |
| idle | 1 | smccc | 1.005 | 1.001 |
| idle | 4 | mmio | 1.000 | 1.002 |
| idle | 4 | smccc | 1.000 | 0.995 |
