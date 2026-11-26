#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Detect wall-clock steps and future inode times outside the product hot path."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'runtime/tests/acceptance'))
from common import digest, identity, save
from run import execute, host, receipt

LANES = ['native', 'smccc-1', 'smccc-4', 'mmio-1', 'mmio-4']
ZERO = ['backward_steps', 'forward_steps', 'future_timestamps',
        'max_backward_ns', 'max_forward_ns', 'max_future_ns']


def audit(out):
    inputs = json.loads((out / 'inputs.json').read_text())
    rows = json.loads((out / 'samples.json').read_text())
    if [r['id'] for r in rows] != LANES:
        raise ValueError('incomplete or duplicate clock matrix')
    build = inputs['build']
    if identity(build['fingerprint']) != build['fingerprint_sha256']:
        raise ValueError('invalid runner build receipt')
    if digest(out / 'original-clock-elf') != inputs['fixture_binary_sha256']:
        raise ValueError('changed original clock ELF')
    for name, expected in inputs['sources'].items():
        if digest(out / name) != expected:
            raise ValueError('changed acceptance source: ' + name)
    passed = []
    for row in rows:
        folder = out / row['id']
        if row != json.loads((folder / 'result.json').read_text()):
            raise ValueError('changed clock result: ' + row['id'])
        for name in ('stdout', 'stderr', 'command.json'):
            if digest(folder / name) != row['logs'][name]:
                raise ValueError('changed clock transcript: ' + row['id'])
        command = json.loads((folder / 'command.json').read_text())
        if command != inputs['commands'][row['id']]:
            raise ValueError('clock command differs from planned input')
        events = [json.loads(line) for line in (folder / 'stdout').read_text().splitlines()]
        summary = events[-1] if events else {}
        valid = (row['status'] == 'completed' and row['exit_code'] == 0
                 and not (folder / 'stderr').read_bytes()
                 and len(events) == 1 and summary.get('event') == 'summary'
                 and set(summary) == {'event', 'samples', *ZERO}
                 and type(summary.get('samples')) is int
                 and summary['samples'] >= inputs['duration_ms'] // 100
                 and all(type(summary.get(k)) is int and summary[k] == 0 for k in ZERO)
                 and row['seconds'] >= inputs['duration_ms'] / 1000)
        passed.append(dict(id=row['id'], status='passed' if valid else 'failed', summary=summary))
    result = dict(cases=passed, passed=sum(r['status'] == 'passed' for r in passed), planned=5,
                  threshold_ns=1000000, duration_ms=inputs['duration_ms'], performance_qualified=False)
    save(out / 'results.json', result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runner', type=Path)
    parser.add_argument('--receipt', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--duration-ms', type=int, default=45000)
    parser.add_argument('--audit', action='store_true')
    args = parser.parse_args(); out = args.output.resolve()
    if args.audit:
        result = audit(out)
        print(json.dumps(result, indent=2))
        return 0 if result['passed'] == 5 else 1
    if not args.runner or not args.receipt or not 25000 <= args.duration_ms <= 120000:
        parser.error('runner, receipt and duration between 25000 and 120000 ms are required')
    if platform.system() != 'Linux' or os.sysconf('SC_PAGE_SIZE') != 4096:
        parser.error('requires actual 4KiB Linux')
    runner = args.runner.resolve(); build = receipt(args.receipt.resolve(), runner)
    out.mkdir(parents=True, exist_ok=False)
    doctor = subprocess.run([runner, 'doctor', '--json', '--require-kvm'], capture_output=True, timeout=15)
    (out / 'doctor.stdout').write_bytes(doctor.stdout); (out / 'doctor.stderr').write_bytes(doctor.stderr)
    if doctor.returncode or doctor.stderr:
        raise RuntimeError('real KVM preflight failed')
    source = ROOT / 'tests/fixtures/clock-consistency.c'; binary = out / 'original-clock-elf'
    sources = {}
    for path in (source, Path(__file__).resolve()):
        shutil.copy2(path, out / path.name); sources[path.name] = digest(path)
    command = [os.environ.get('CC', 'cc'), '-O2', '-static', '-Wall', '-Wextra', '-Werror',
               '-Wl,-z,max-page-size=4096', '-Wl,-z,common-page-size=4096', str(source), '-o', str(binary)]
    with (out / 'fixture.build.log').open('wb') as log:
        subprocess.run(command, check=True, stdout=log, stderr=subprocess.STDOUT)
    root = out / 'rootfs'; (root / 'test').mkdir(parents=True); (root / 'work').mkdir()
    shutil.copy2(binary, root / 'test/clock-probe')
    commands = {}
    for lane in LANES:
        environment = {'TRAMES_KVM_TRANSPORT': lane.split('-')[0]} if lane != 'native' else {}
        if lane == 'native':
            argv = [binary, str(args.duration_ms), root / 'work' / lane]
        else:
            argv = [runner, 'run', '--cpus', lane.split('-')[1], '--rootfs', root,
                    '--timeout-ms', str(args.duration_ms + 15000), '--', '/test/clock-probe',
                    str(args.duration_ms), '/work/' + lane]
        commands[lane] = dict(argv=[str(v) for v in argv], environment=environment)
    save(out / 'inputs.json', dict(schema_version=1, host=host(), build=build,
         fixture_binary_sha256=digest(binary), compiler_command=command, sources=sources,
         commands=commands, duration_ms=args.duration_ms, started_realtime_ns=time.time_ns()))

    def run(lane):
        folder = out / lane; folder.mkdir(); spec = commands[lane]
        save(folder / 'command.json', spec)
        result = execute(spec['argv'], os.environ | spec['environment'], folder,
                         args.duration_ms / 1000 + 20)
        row = dict(id=lane, **result, logs={name: digest(folder / name)
                   for name in ('stdout', 'stderr', 'command.json')})
        save(folder / 'result.json', row)
        print(lane, result['status'], result['exit_code'], flush=True)
        return row

    with ThreadPoolExecutor(max_workers=5) as pool:
        rows = list(pool.map(run, LANES))
    save(out / 'samples.json', rows)
    if digest(root / 'test/clock-probe') != digest(binary):
        raise ValueError('guest fixture changed during execution')
    result = audit(out)
    print(f"Clock consistency: {result['passed']}/5 passed", flush=True)
    return 0 if result['passed'] == 5 else 1


if __name__ == '__main__':
    raise SystemExit(main())
