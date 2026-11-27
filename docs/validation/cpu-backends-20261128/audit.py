#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Offline consistency audit. Never runs a VM, executable or workload."""
from copy import deepcopy
import gzip
import hashlib
import importlib.util
import json
from pathlib import Path
import statistics

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
spec = importlib.util.spec_from_file_location('cpu_backend_controller', ROOT/'scripts/benchmark-cpu-backends.py')
controller = importlib.util.module_from_spec(spec)
spec.loader.exec_module(controller)


def validate(files, receipt, summary):
    assert len(files) == 18
    expected_orders = receipt['orders']
    seen = set()
    reference = {}
    grouped = {}
    for name, records in files.items():
        _, group, position, backend = Path(name).stem.split('-')
        group, position = int(group), int(position)
        assert expected_orders[group-1][position-1] == backend
        assert (group, position) not in seen
        seen.add((group, position))
        for row in controller.audit(records):
            assert row['backend'] == backend and row['mode'] == 'worker'
            identity = {key:row[key] for key in ('mode','words','trips','iterations',
                'logical_guest_instructions','final_data','final_x0','final_pc')}
            identity['states'] = [(s['x0'], s['pc']) for s in row['samples']]
            assert reference.setdefault(row['workload'], identity) == identity
            grouped.setdefault((row['workload'], backend), []).append(row)
    assert len(grouped) == 18
    assert len(summary) == 6
    measured = 0
    for row in summary:
        identity = reference[row['workload']].copy()
        identity.pop('states')
        assert all(row[key] == value for key,value in identity.items())
        for backend, stored in row['backends'].items():
            rows = grouped[(row['workload'], backend)]
            assert len(rows) == 6
            medians = [r['median_ns'] for r in rows]
            assert stored['process_medians_ns'] == medians
            assert stored['median_ns'] == statistics.median(medians)
            assert stored['ns_per_syscall_return'] == statistics.median(medians)/row['trips']
            assert stored['timed_yields'] == [[s['yields'] for s in r['samples'][1:]] for r in rows]
            measured += sum(len(r['samples'])-1 for r in rows)
    assert measured == 540
    return measured


def main():
    receipt = json.loads(gzip.decompress((HERE/'receipt.json.gz').read_bytes()))
    summary = json.loads((HERE/'summary.json').read_text())
    # Preserve process order rather than lexicographically regrouping lanes.
    files = {}
    for group, order in enumerate(receipt['orders'],1):
        for position,backend in enumerate(order,1):
            name=f'round-{group}-{position}-{backend}.jsonl'
            files[name]=[json.loads(line) for line in (HERE/name).read_text().splitlines()]
    measured = validate(files,receipt,summary)
    digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
    assert digest(ROOT/'scripts/benchmark-cpu-backends.py') == receipt['controller_sha256']
    for name,value in receipt['source_sha256'].items():
        if name.startswith(('tools/cpu-backends/','crates/trames-kvm/')) or name in ('Cargo.toml','Cargo.lock','rust-toolchain.toml'):
            assert digest(ROOT/name)==value,name
    negative=[]
    cases=['missing process','mismatched original instruction','changed canonical result','false median','false KVM exact count']
    for case in cases:
        bad=deepcopy(files)
        name=next(iter(bad))
        if case=='missing process': bad.pop(name)
        elif case=='mismatched original instruction': bad[name][0]['words'][0]^=1
        elif case=='changed canonical result': bad[name][0]['final_data']^=1
        elif case=='false median': bad[name][0]['median_ns']+=1
        else:
            name=next(n for n in bad if n.endswith('-kvm.jsonl'))
            next(r for r in bad[name] if r['kind']=='workload')['exact_instructions']=True
        try:
            validate(bad,receipt,summary)
        except AssertionError:
            negative.append(case)
        else:
            raise AssertionError('negative evidence accepted: '+case)
    print(json.dumps({'passed':True,'processes':18,'workloads':6,'timed_samples':measured,
        'negative_cases_rejected':negative,'scope':'offline raw/work/result/median/source-binding audit'},indent=2))


if __name__=='__main__':
    main()
