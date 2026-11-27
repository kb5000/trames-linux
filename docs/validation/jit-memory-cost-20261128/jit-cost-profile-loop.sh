#!/bin/sh
set -eu
bench_mode=$1
bench_round=0
while [ "$bench_round" -lt 24 ]; do
  /mnt/trames-output/linux-desktop-target/release/trames-cpu-backends jit "$bench_mode" >> /mnt/trames-output/jit-cost-profile-$bench_mode.jsonl
  bench_round=$((bench_round+1))
done
