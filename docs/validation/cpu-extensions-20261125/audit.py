#!/usr/bin/env python3
"""Offline source/receipt/raw-result checks; does not execute a guest."""
from pathlib import Path
import json
import os
import re
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(ROOT / 'runtime/tests/acceptance'))
from common import digest, identity


def read(path):
    return json.loads(path.read_text())


permissions = read(HERE / 'source-permissions.json')


def source_inputs(name, root):
    # Replay the build's group-write metadata without modifying this checkout.
    # Bytes, symlink targets and executable bits must match the current source.
    paths = subprocess.check_output(['git', 'ls-files', '-c', '-o', '--exclude-standard', '-z'], cwd=root).split(b'\0')
    entries = []
    for raw in sorted(set(paths)):
        if not raw:
            continue
        filename = os.fsdecode(raw)
        if filename.startswith(('docs/', 'tests/', '.github/')) or filename.endswith('.md'):
            continue
        p = root / filename
        if p.is_symlink():
            entries.append([filename, 'link', os.readlink(p)])
        elif p.is_file():
            actual = p.stat().st_mode & 0o777
            mode = permissions[name].get(filename, actual)
            assert mode & 0o111 == actual & 0o111, filename
            entries.append([filename, mode, digest(p)])
        elif p.is_dir() and (p / '.git').exists():
            entries.append([filename, 'submodule', source_inputs(filename, p)])
        elif not p.exists():
            entries.append([filename, 'deleted'])
    return identity(entries)


build = read(HERE / 'trames-cpu-extensions-build-v5.json')
assert identity(build['fingerprint']) == build['fingerprint_sha256']
for name, path in [('product', ROOT), ('runtime', ROOT / 'runtime')]:
    assert source_inputs(name, path) == build['sources'][name]['inputs_sha256'], name
assert not any(source['dirty'] for source in build['sources'].values())
final = HERE / 'trames-cpu-extensions-validation-v5'
inputs = read(final / 'inputs.json')
assert inputs['build'] == build
assert digest(ROOT / 'tests/fixtures/cpu-extensions.c') == inputs['fixture_source_sha256']
assert digest(ROOT / 'scripts/validate-cpu.py') == inputs['controller_sha256']
assert inputs['performance_qualified'] is False
profile = inputs['profile']
assert profile['hwcap'] == 783546363 and profile['hwcap2'] == 562954251559297
assert len(profile['features'].split()) == 32
rows = read(final / 'results.json')
expected = {f'native-{mode}' for mode in ['instructions', 'state', 'ptrace']}
expected |= {f'{mode}-{transport}-{workers}' for mode in ['instructions', 'state', 'ptrace']
             for transport in ['smccc', 'mmio'] for workers in [1, 4]}
assert rows['passed'] == rows['planned'] == 15
assert {row['id'] for row in rows['cases']} == expected
for row in rows['cases']:
    assert row['status'] == 'passed' and row['exit_code'] == 0
    mode = row['id'].split('-')[1 if row['id'].startswith('native-') else 0]
    assert (final / row['id'] / 'stdout').read_bytes() == (final / ('native-' + mode) / 'stdout').read_bytes()
    assert not (final / row['id'] / 'stderr').read_bytes()
apple = read(HERE / 'apple-final/results.json')
assert apple['runtime'] == build['sources']['runtime']['commit']
assert apple['fixture_binary_sha256'] == inputs['fixture_binary_sha256']
assert len(apple['cases']) == 4
for row in apple['cases']:
    assert row['passed'] and row['exit_code'] == 0
    mode = row['id'].split('-')[1]
    assert (HERE / 'apple-final' / row['id'] / 'stdout').read_bytes() == (final / ('native-' + mode) / 'stdout').read_bytes()
hardware = 0
for transport in ['smccc', 'mmio']:
    log = (HERE / f'trames-cpu-extensions-hardware-{transport}.log').read_text()
    counts = [int(value) for value in re.findall(r'test result: ok\. (\d+) passed; 0 failed', log)]
    assert counts == [16, 7]
    hardware += sum(counts)
cli = 0
for transport in ['smccc', 'mmio']:
    for workers in [1, 4]:
        lane = HERE / f'applications/kvm-{transport}-{workers}'
        results = read(lane / 'results.json')
        assert len(results['cases']) == 12
        for row in results['cases']:
            assert row['status'] == 'passed' and not row['reused']
            folder = lane / 'cases' / row['id']
            for stream, value in row['logs'].items():
                assert digest(folder / stream) == value
            for name, value in row.get('artifacts', {}).items():
                assert digest(folder / 'artifacts' / name.lstrip('/')) == value
            cli += 1
assert read(HERE / 'trames-cpu-extensions-validation-v4/results.json')['passed'] == 11
print(json.dumps(dict(cpu=15, hvf=4, reused_hardware=hardware, cli_before_final_fp_fix=cli,
                      source_inputs_match=True, historical_failures_retained=True)))
