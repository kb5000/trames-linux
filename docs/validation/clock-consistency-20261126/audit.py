#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Offline audit; requires a trames-linux checkout with its pinned Runtime."""
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(ROOT / 'runtime/tests/acceptance'))
from common import digest, identity, save


def load(path):
    return json.loads(path.read_text())


def audit():
    index = load(HERE / 'evidence-index.json')
    assert digest(HERE / 'raw-evidence.tar.gz') == index['archive_sha256']
    for name, expected in index['sources'].items():
        assert digest(ROOT / name) == expected, name
    with tempfile.TemporaryDirectory(prefix='trames-clock-audit-') as temporary:
        raw = Path(temporary)
        with tarfile.open(HERE / 'raw-evidence.tar.gz') as archive:
            for member in archive.getmembers():
                path = Path(member.name)
                assert not path.is_absolute() and '..' not in path.parts
                assert member.isdir() or member.isfile()
            archive.extractall(raw)
        for name, expected in index['files'].items():
            assert digest(raw / name) == expected, name

        before = load(raw / 'before/results.json')
        assert [r['label'] for r in before] == ['native', 'smccc', 'mmio']
        for row in before:
            folder = raw / 'before' / row['label']
            events = [json.loads(line) for line in (folder / 'stdout').read_text().splitlines()]
            assert events[-1] == row['summary'] and not (folder / 'stderr').read_bytes()
            assert row['exit_code'] == 1
            assert row['summary']['backward_steps'] == 3
            assert row['summary']['future_timestamps'] == 3
            assert row['summary']['forward_steps'] == 0
            assert 100000000 < row['summary']['max_backward_ns'] < 300000000
            assert sum(e['event'] == 'backward-step' for e in events) == 3
        initial = load(raw / 'after-initial/results.json')
        assert len(initial) == 5 and len({r['label'] for r in initial}) == 5
        for row in initial:
            assert row['exit_code'] == 0
            assert row['summary']['samples'] > 1000
            assert all(row['summary'][name] == 0 for name in (
                'backward_steps', 'forward_steps', 'future_timestamps',
                'max_backward_ns', 'max_forward_ns', 'max_future_ns'))
            assert json.loads((raw / 'after-initial' / row['label'] / 'stdout').read_text()) == row['summary']
            assert not (raw / 'after-initial' / row['label'] / 'stderr').read_bytes()
        diagnostic = load(raw / 'policy/diagnostic-identity.json')
        assert diagnostic['before']['sha256'] == diagnostic['after-initial']['sha256']
        assert digest(raw / 'before/original-clock-elf') == diagnostic['before']['sha256']

        policy = load(raw / 'policy/final.json')
        assert set(policy['agent_capabilities']) == {'CapEff', 'CapPrm', 'CapBnd'}
        assert all(int(value, 16) & (1 << 25) == 0 for value in policy['agent_capabilities'].values())
        assert policy['configurer_sha256'] == digest(raw / 'policy/configure-lima-clock.sh')
        assert 'NTPSynchronized=yes' in policy['commands'][0]['stdout']
        assert 'ActiveState=active' in policy['commands'][3]['stdout']
        cleanup = load(raw / 'policy/cleanup.json')
        assert cleanup['immutable_master_unchanged'] is True and cleanup['immutable_master_files'] == 169
        assert len(cleanup['removed_only_this_task_rootfs']) == 5
        repeat = load(raw / 'policy/idempotent-apply.json')
        assert repeat['exit_code'] == 0 and repeat['pid_before'] == repeat['pid_after']
        assert 'system time synchronized with host' in load(raw / 'policy/diagnostic-journal.json')['stdout']

        qualified = raw / 'qualified'
        spec = importlib.util.spec_from_file_location('frozen_clock_gate', qualified / 'validate-clock.py')
        gate = importlib.util.module_from_spec(spec); spec.loader.exec_module(gate)
        assert gate.audit(qualified)['passed'] == 5
        assert digest(qualified / 'original-clock-elf') == diagnostic['qualified']['sha256']
        negative = []
        for name in ('changed-transcript', 'incomplete-matrix', 'future-timestamp'):
            target = raw / name; shutil.copytree(qualified, target)
            if name == 'changed-transcript':
                with (target / 'native/stdout').open('ab') as stream:
                    stream.write(b'changed\n')
            elif name == 'incomplete-matrix':
                save(target / 'samples.json', load(target / 'samples.json')[:-1])
            else:
                path = target / 'native/stdout'; value = load(path)
                value['future_timestamps'] = 1; value['max_future_ns'] = 150000000; save(path, value)
                rows = load(target / 'samples.json'); rows[0]['logs']['stdout'] = digest(path)
                save(target / 'native/result.json', rows[0]); save(target / 'samples.json', rows)
            try:
                rejected = gate.audit(target)['passed'] != 5
            except ValueError:
                rejected = True
            assert rejected, name
            negative.append(name)

        builds = raw / 'builds'; inputs = load(builds / 'inputs.json'); rows = load(builds / 'samples.json')
        assert inputs['immutable_files'] == load(builds / 'prior-inputs.json')['immutable_files']
        assert digest(builds / 'controller.py') == inputs['controller_sha256']
        assert digest(builds / 'original-performance-elf') == inputs['fixture_binary_sha256']
        for build in inputs['builds'].values():
            assert identity(build['fingerprint']) == build['fingerprint_sha256']
        expected = [f'{lane}-{workers}' for workers in (1, 4) for lane in inputs['lanes']]
        assert [r['id'] for r in rows] == expected and len(rows) == 10
        native = {}
        for row in rows:
            folder = builds / row['id']; assert load(folder / 'result.json') == row
            for name, expected_hash in row['logs'].items():
                assert digest(folder / name) == expected_hash
            assert row['status'] == 'passed' and row['exit_code'] == 0
            assert not (folder / 'stderr').read_bytes()
            assert load(folder / 'stdout') == row['work']
            value = row['work']; workers = int(row['id'].split('-')[-1])
            assert value['mode'] == 'build' and value['iterations'] == 12
            assert value['concurrency'] == value['tasks'] == workers and value['work_ns'] > 0
            assert len(row['objects']) == 12
            if row['id'].startswith('native-'):
                native[workers] = row
            assert row['objects'] == native[workers]['objects']
            assert value['checksum'] == native[workers]['work']['checksum']
        assert load(builds / 'results.json')['cases'] == rows
        warning = raw / 'original-warning'; old = load(warning / 'result.json')
        assert old['work']['checksum'] == native[1]['work']['checksum']
        assert old['exit_code'] == 0 and b'Clock skew detected' in (warning / 'stderr').read_bytes()
        for name, expected_hash in old['logs'].items():
            assert digest(warning / name) == expected_hash
        result = dict(passed=True, before_failed=3, same_elf_after_passed=5,
                      qualified_clock_passed=5, gcc_make_passed=10,
                      negative_audit_rejections=negative, original_warning_retained=True,
                      runtime_changes=False, performance_qualified=False)
        save(HERE / 'audit-result.json', result)
        print(json.dumps(result, indent=2))


if __name__ == '__main__':
    audit()
