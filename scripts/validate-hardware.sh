#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
binary="${TRAMES_BINARY:-target/release/trames}"
output="${TRAMES_VALIDATION_OUTPUT:-target/validation}"
mkdir -p "$output"
"$binary" doctor --json --require-kvm > "$output/doctor.json"
for transport in smccc mmio; do
  "$binary" probe --transport "$transport" --iterations 10000 --json > "$output/$transport.json"
done
python3 - "$output" <<'PY'
from pathlib import Path
import json, sys
root = Path(sys.argv[1])
for transport in ('smccc', 'mmio'):
    p = json.loads((root / f'{transport}.json').read_text())['probe']
    assert p['transport'] == transport and p['hardware_executed']
    assert p['host']['host_page_size'] == p['guest_page_size'] == 4096
    for name in ('registers_preserved','canonical_memory','permission_fault','permission_resume','unmapped_fault','el1_isolated','deadline_interrupted'):
        assert p[name], (transport, name)
    assert p['syscall_round_trips'] == 10000
    assert p['kvm_run_calls'] >= p['syscall_round_trips']
    print(transport, 'hardware checks passed;', round(p['ns_per_round_trip'], 1), 'ns per validated round trip')
PY
