# Paired original workload baseline

Audit: **passed**.
108 measured executions; calibration/warmup excluded.

Medians are milliseconds. Ratio is KVM/native work time (lower is better).
Wall and CPU include startup; sampled memory may miss short peaks.
This host/configuration only; caches retained; no bare-metal claim.

| Workload | Workers | Lane | Work ms | Wall ms | CPU ms | Work/native |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| build | 1 | mmio | 24479.601 | 24550.084 | 24294.779 | 7.846 |
| build | 1 | native | 3119.920 | 3121.305 | 3083.262 | 1.000 |
| build | 1 | smccc | 22523.362 | 22590.336 | 22349.110 | 7.219 |
| build | 4 | mmio | 32111.755 | 32203.831 | 124022.720 | 32.472 |
| build | 4 | native | 988.899 | 989.819 | 3819.980 | 1.000 |
| build | 4 | smccc | 30692.875 | 30778.786 | 117684.396 | 31.037 |
| cpu | 1 | mmio | 58.159 | 74.861 | 73.610 | 1.066 |
| cpu | 1 | native | 54.577 | 55.025 | 54.570 | 1.000 |
| cpu | 1 | smccc | 57.983 | 72.655 | 71.927 | 1.062 |
| cpu | 4 | mmio | 90.491 | 127.661 | 273.119 | 1.569 |
| cpu | 4 | native | 57.665 | 58.117 | 229.623 | 1.000 |
| cpu | 4 | smccc | 91.434 | 126.180 | 270.689 | 1.586 |
| getpid | 1 | mmio | 470.808 | 488.086 | 485.316 | 371.091 |
| getpid | 1 | native | 1.269 | 1.641 | 1.550 | 1.000 |
| getpid | 1 | smccc | 371.240 | 387.639 | 385.765 | 292.612 |
| getpid | 4 | mmio | 798.636 | 842.115 | 2398.911 | 607.844 |
| getpid | 4 | native | 1.314 | 1.837 | 5.443 | 1.000 |
| getpid | 4 | smccc | 628.923 | 671.679 | 1884.714 | 478.675 |
| idle | 1 | mmio | 1003.972 | 1041.948 | 40.303 | 0.994 |
| idle | 1 | native | 1010.228 | 1011.612 | 2.071 | 1.000 |
| idle | 1 | smccc | 1015.352 | 1056.320 | 44.142 | 1.005 |
| idle | 4 | mmio | 1026.694 | 1099.193 | 87.170 | 1.017 |
| idle | 4 | native | 1009.549 | 1011.629 | 3.251 | 1.000 |
| idle | 4 | smccc | 1027.305 | 1099.118 | 88.479 | 1.018 |
| mmap | 1 | mmio | 143.622 | 159.492 | 158.647 | 162.327 |
| mmap | 1 | native | 0.885 | 1.378 | 1.252 | 1.000 |
| mmap | 1 | smccc | 125.813 | 143.085 | 139.598 | 142.198 |
| mmap | 4 | mmio | 681.452 | 721.729 | 1719.307 | 80.676 |
| mmap | 4 | native | 8.447 | 9.013 | 12.831 | 1.000 |
| mmap | 4 | smccc | 652.025 | 685.937 | 1610.165 | 77.192 |
| node | 1 | mmio | 51.195 | 3667.455 | 3650.588 | 1.899 |
| node | 1 | native | 26.953 | 110.967 | 109.182 | 1.000 |
| node | 1 | smccc | 49.652 | 3373.499 | 3344.874 | 1.842 |
| node | 4 | mmio | 156.158 | 8979.583 | 24925.602 | 5.248 |
| node | 4 | native | 29.753 | 200.705 | 357.530 | 1.000 |
| node | 4 | smccc | 160.733 | 8479.414 | 23714.328 | 5.402 |
