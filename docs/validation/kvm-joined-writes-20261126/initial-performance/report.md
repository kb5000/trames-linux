# Paired original workload baseline

Audit: **passed**.
180 measured executions; calibration/warmup excluded.

Medians are milliseconds. Ratio is KVM/native work time (lower is better).
Wall and CPU include startup; sampled memory may miss short peaks.
This host/configuration only; caches retained; no bare-metal claim.

| Workload | Workers | Lane | Work ms | Wall ms | CPU ms | Work/native |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| build | 1 | baseline-mmio | 23932.499 | 24005.086 | 23607.910 | 7.688 |
| build | 1 | baseline-smccc | 22055.354 | 22120.951 | 21782.951 | 7.085 |
| build | 1 | candidate-mmio | 22369.465 | 22440.552 | 22141.081 | 7.186 |
| build | 1 | candidate-smccc | 20689.481 | 20755.329 | 20460.424 | 6.646 |
| build | 1 | native | 3113.041 | 3113.882 | 3083.785 | 1.000 |
| build | 4 | baseline-mmio | 31830.733 | 31927.671 | 121426.444 | 32.085 |
| build | 4 | baseline-smccc | 30562.004 | 30645.676 | 116573.384 | 30.806 |
| build | 4 | candidate-mmio | 30158.248 | 30248.666 | 114804.116 | 30.399 |
| build | 4 | candidate-smccc | 30146.383 | 30232.474 | 114736.153 | 30.387 |
| build | 4 | native | 992.073 | 993.101 | 3807.736 | 1.000 |
| cpu | 1 | baseline-mmio | 57.360 | 73.540 | 72.840 | 1.057 |
| cpu | 1 | baseline-smccc | 57.415 | 72.033 | 71.636 | 1.058 |
| cpu | 1 | candidate-mmio | 57.288 | 76.056 | 72.320 | 1.056 |
| cpu | 1 | candidate-smccc | 57.328 | 71.330 | 71.038 | 1.056 |
| cpu | 1 | native | 54.266 | 54.665 | 54.532 | 1.000 |
| cpu | 4 | baseline-mmio | 87.367 | 126.312 | 273.291 | 1.520 |
| cpu | 4 | baseline-smccc | 83.632 | 122.698 | 272.474 | 1.455 |
| cpu | 4 | candidate-mmio | 86.074 | 123.139 | 275.751 | 1.497 |
| cpu | 4 | candidate-smccc | 86.696 | 120.894 | 269.255 | 1.508 |
| cpu | 4 | native | 57.484 | 58.001 | 229.615 | 1.000 |
| getpid | 1 | baseline-mmio | 450.776 | 467.103 | 466.498 | 397.539 |
| getpid | 1 | baseline-smccc | 353.024 | 369.188 | 368.562 | 311.332 |
| getpid | 1 | candidate-mmio | 455.297 | 471.938 | 471.342 | 401.526 |
| getpid | 1 | candidate-smccc | 360.754 | 375.654 | 375.149 | 318.149 |
| getpid | 1 | native | 1.134 | 1.664 | 1.531 | 1.000 |
| getpid | 4 | baseline-mmio | 785.925 | 832.574 | 2358.202 | 590.976 |
| getpid | 4 | baseline-smccc | 615.292 | 660.839 | 1850.907 | 462.668 |
| getpid | 4 | candidate-mmio | 785.877 | 831.820 | 2360.994 | 590.940 |
| getpid | 4 | candidate-smccc | 614.800 | 656.400 | 1847.248 | 462.298 |
| getpid | 4 | native | 1.330 | 1.896 | 5.553 | 1.000 |
| memory | 1 | baseline-mmio | 17.696 | 215.460 | 215.022 | 1.164 |
| memory | 1 | baseline-smccc | 17.444 | 195.727 | 195.297 | 1.147 |
| memory | 1 | candidate-mmio | 17.117 | 213.745 | 213.262 | 1.126 |
| memory | 1 | candidate-smccc | 16.829 | 190.617 | 190.136 | 1.107 |
| memory | 1 | native | 15.205 | 16.823 | 16.662 | 1.000 |
| memory | 4 | baseline-mmio | 175.987 | 1110.131 | 1292.618 | 6.340 |
| memory | 4 | baseline-smccc | 70.222 | 886.000 | 936.761 | 2.530 |
| memory | 4 | candidate-mmio | 118.654 | 995.103 | 1104.918 | 4.274 |
| memory | 4 | candidate-smccc | 240.051 | 1044.782 | 1275.732 | 8.648 |
| memory | 4 | native | 27.759 | 33.767 | 109.630 | 1.000 |
| mmap | 1 | baseline-mmio | 146.972 | 163.650 | 162.882 | 158.801 |
| mmap | 1 | baseline-smccc | 129.002 | 143.736 | 143.048 | 139.385 |
| mmap | 1 | candidate-mmio | 147.033 | 163.114 | 162.348 | 158.868 |
| mmap | 1 | candidate-smccc | 128.979 | 143.997 | 143.218 | 139.361 |
| mmap | 1 | native | 0.926 | 1.525 | 1.360 | 1.000 |
| mmap | 4 | baseline-mmio | 672.381 | 712.550 | 1700.844 | 81.432 |
| mmap | 4 | baseline-smccc | 642.808 | 676.105 | 1639.407 | 77.850 |
| mmap | 4 | candidate-mmio | 678.144 | 720.400 | 1720.436 | 82.130 |
| mmap | 4 | candidate-smccc | 636.149 | 674.323 | 1628.631 | 77.044 |
| mmap | 4 | native | 8.257 | 8.891 | 12.507 | 1.000 |
| node | 1 | baseline-mmio | 51.005 | 3663.258 | 3609.486 | 1.894 |
| node | 1 | baseline-smccc | 50.142 | 3366.800 | 3337.128 | 1.862 |
| node | 1 | candidate-mmio | 46.490 | 2968.070 | 2902.209 | 1.726 |
| node | 1 | candidate-smccc | 45.455 | 2623.564 | 2591.653 | 1.688 |
| node | 1 | native | 26.928 | 111.520 | 110.092 | 1.000 |
| node | 4 | baseline-mmio | 153.849 | 8588.949 | 23234.659 | 4.451 |
| node | 4 | baseline-smccc | 126.151 | 8015.863 | 21989.555 | 3.649 |
| node | 4 | candidate-mmio | 146.384 | 7632.186 | 19966.731 | 4.235 |
| node | 4 | candidate-smccc | 154.360 | 7250.537 | 19220.537 | 4.465 |
| node | 4 | native | 34.568 | 204.745 | 369.264 | 1.000 |

## Candidate / baseline time ratios

| Workload | Workers | Transport | Work ratio | Wall ratio |
| --- | ---: | --- | ---: | ---: |
| build | 1 | mmio | 0.935 | 0.935 |
| build | 1 | smccc | 0.938 | 0.938 |
| build | 4 | mmio | 0.947 | 0.947 |
| build | 4 | smccc | 0.986 | 0.987 |
| cpu | 1 | mmio | 0.999 | 1.034 |
| cpu | 1 | smccc | 0.998 | 0.990 |
| cpu | 4 | mmio | 0.985 | 0.975 |
| cpu | 4 | smccc | 1.037 | 0.985 |
| getpid | 1 | mmio | 1.010 | 1.010 |
| getpid | 1 | smccc | 1.022 | 1.018 |
| getpid | 4 | mmio | 1.000 | 0.999 |
| getpid | 4 | smccc | 0.999 | 0.993 |
| memory | 1 | mmio | 0.967 | 0.992 |
| memory | 1 | smccc | 0.965 | 0.974 |
| memory | 4 | mmio | 0.674 | 0.896 |
| memory | 4 | smccc | 3.418 | 1.179 |
| mmap | 1 | mmio | 1.000 | 0.997 |
| mmap | 1 | smccc | 1.000 | 1.002 |
| mmap | 4 | mmio | 1.009 | 1.011 |
| mmap | 4 | smccc | 0.990 | 0.997 |
| node | 1 | mmio | 0.911 | 0.810 |
| node | 1 | smccc | 0.907 | 0.779 |
| node | 4 | mmio | 0.951 | 0.889 |
| node | 4 | smccc | 1.224 | 0.905 |
