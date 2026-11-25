#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Compare real keyring/pidfd ABI behavior with the native Linux kernel."""
import hashlib
import json
import os
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1]
runner = Path(os.environ.get('TRAMES_BINARY', root/'target/release/trames')).resolve()
output = Path(os.environ.get('TRAMES_VALIDATION_OUTPUT', root/'target/validation/process-resources')).resolve()
output.mkdir(parents=True, exist_ok=False)
source = root/'runtime/tests/fixtures/process-resources.c'
fixture = output/'process-resources'
subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', '-O2', '-static', str(source), '-o', str(fixture)], check=True)
results = []
for mode in ['keyrings', 'pidfd', 'clone-pidfd']:
    reference_passed = False
    for backend, transport, cpus in [('native', None, None)] + [('kvm', t, c) for t in ['smccc', 'mmio'] for c in [1, 4]]:
        if backend == 'kvm' and not reference_passed:
            results.append(dict(mode=mode, backend=backend, transport=transport, cpus=cpus,
                                passed=False, status='reference_failed', reason='native reference did not pass; guest not executed'))
            continue
        argv = [str(fixture), mode]
        environment = dict(os.environ)
        if backend == 'kvm':
            environment['TRAMES_KVM_TRANSPORT'] = transport
            argv = [str(runner), 'run', '--cpus', str(cpus), '--timeout-ms', '20000', '--', *argv]
        try:
            result = subprocess.run(argv, env=environment, capture_output=True, timeout=25)
            row = dict(mode=mode, backend=backend, transport=transport, cpus=cpus, code=result.returncode,
                       stdout=result.stdout.decode(errors='replace'), stderr=result.stderr.decode(errors='replace'))
            row['passed'] = result.returncode == 0 and result.stdout == (mode+': ok\n').encode() and not result.stderr
        except subprocess.TimeoutExpired:
            row = dict(mode=mode, backend=backend, transport=transport, cpus=cpus, passed=False, reason='timeout')
        row['status'] = 'passed' if row['passed'] else 'failed'
        if backend == 'native':
            reference_passed = row['passed']
        results.append(row)
        (output/'results.json').write_text(json.dumps(dict(source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(), cases=results), indent=2)+'\n')
        print(mode, backend, transport, cpus, 'passed' if row['passed'] else 'FAILED', flush=True)
(output/'results.json').write_text(json.dumps(dict(source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(), cases=results), indent=2)+'\n')
if not all(row['passed'] for row in results):
    raise SystemExit('process resource regressions failed; see results.json')
