#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Run balanced matched CPU payloads; retain every raw sample and source hash."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess

ORDERS = [
    ('interpreter', 'jit', 'kvm'), ('jit', 'kvm', 'interpreter'),
    ('kvm', 'interpreter', 'jit'), ('kvm', 'jit', 'interpreter'),
    ('jit', 'interpreter', 'kvm'), ('interpreter', 'kvm', 'jit'),
]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def audit(records):
    workloads = [r for r in records if r['kind'] == 'workload']
    assert len(workloads) == 6
    for row in workloads:
        samples = row['samples']
        assert len(samples) == 6 and samples[0]['warmup']
        assert not any(s['warmup'] for s in samples[1:])
        assert row['median_ns'] == sorted(s['ns'] for s in samples[1:])[2]
        for sample in samples:
            assert sample['syscalls'] == row['trips']
            assert sample['calls'] == sample['syscalls'] + sample['yields']
            assert sample['pc'] == row['final_pc']
            observed = sample['observed_instructions']
            assert observed == (row['logical_guest_instructions'] if row['exact_instructions'] else None)
        if row['backend'] == 'jit':
            assert row['diagnostics']['fallback_instructions'] == 0
    if workloads[0]['backend'] == 'kvm':
        host = next(r for r in records if r['kind'] == 'host')['doctor']
        assert host['kvm_api_version'] == 12 and host['smccc_forwarding']
        assert host['host_page_size'] == 4096 and host['backend_implemented']
    return workloads


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--linux-revision', required=True)
    parser.add_argument('--runtime-revision', required=True)
    parser.add_argument('--mode', choices=['worker', 'direct'], default='worker')
    parser.add_argument('--cpu', type=int, default=2)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    assert not list(args.output.glob('round-*.jsonl')), 'keep cohorts in separate directories'
    env = dict(os.environ, TRAMES_KVM_TRANSPORT='smccc')
    groups = {}
    reference = {}
    for group, order in enumerate(ORDERS, 1):
        for position, backend in enumerate(order, 1):
            command = ['taskset', '-c', str(args.cpu), str(args.binary.resolve()), backend, args.mode]
            result = subprocess.run(command, env=env, capture_output=True, timeout=90)
            path = args.output / f'round-{group}-{position}-{backend}.jsonl'
            path.write_bytes(result.stdout)
            (path.with_suffix('.stderr')).write_bytes(result.stderr)
            assert result.returncode == 0, (command, result.returncode, path.with_suffix('.stderr'))
            rows = audit([json.loads(line) for line in result.stdout.splitlines()])
            for row in rows:
                name = row['workload']
                identity = {key: row[key] for key in ('mode', 'words', 'trips', 'iterations',
                    'logical_guest_instructions', 'final_data', 'final_x0', 'final_pc')}
                identity['states'] = [(s['x0'], s['pc']) for s in row['samples']]
                assert reference.setdefault(name, identity) == identity, (name, backend, 'work mismatch')
                groups.setdefault(name, {}).setdefault(backend, []).append(row)
            print(f'group {group}/6 {backend}: exact payload/state audit passed', flush=True)
    summary = []
    for name, backends in groups.items():
        item = {'workload': name, **reference[name]}
        item.pop('states')
        item['backends'] = {}
        for backend, rows in backends.items():
            assert len(rows) == 6
            medians = [row['median_ns'] for row in rows]
            item['backends'][backend] = {
                'median_ns': statistics.median(medians), 'process_medians_ns': medians,
                'ns_per_syscall_return': statistics.median(medians) / item['trips'],
                'timed_yields': [[s['yields'] for s in r['samples'][1:]] for r in rows],
            }
        summary.append(item)
    (args.output/'summary.json').write_text(json.dumps(summary, indent=2)+'\n')
    inputs = {}
    for directory in [args.source/'crates/trames-kvm', args.source/'tools/cpu-backends', args.source/'runtime/crates']:
        for p in directory.rglob('*'):
            if p.is_file() and p.suffix in ('.rs', '.S', '.c', '.h', '.toml', '.lock'):
                inputs[str(p.relative_to(args.source))] = sha(p)
    for rel in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'runtime/Cargo.toml', 'runtime/Cargo.lock']:
        inputs[rel] = sha(args.source/rel)
    receipt = {
        'linux_revision': args.linux_revision, 'runtime_revision': args.runtime_revision,
        'binary_sha256': sha(args.binary), 'controller_sha256': sha(Path(__file__)),
        'source_sha256': inputs, 'source_overrides_product_runtime_pin': True,
        'mode': args.mode, 'cpu_affinity': [args.cpu], 'orders': ORDERS,
        'transport': 'forced SMCCC, unavailable transport fails',
        'uname': subprocess.check_output(['uname', '-a'], text=True).strip(),
        'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
        'profile': 'release, thin LTO, overflow checks, incremental disabled',
        'scope': 'finite payload / worker CPU boundary including normal yields; no Linux syscall handler',
        'timer': 'KVM production 2ms assignment deadline retained across short SVCs',
    }
    (args.output/'receipt.json').write_text(json.dumps(receipt, indent=2)+'\n')
    print(json.dumps([{'workload':r['workload'], 'medians_ns':{b:s['median_ns'] for b,s in r['backends'].items()}} for r in summary], indent=2))


if __name__ == '__main__':
    main()
