#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Audit frozen budget-reuse evidence without executing a VM or workload."""
from pathlib import Path
import csv
import importlib.util
import json
import re
import sys
import tarfile
import tempfile

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
PREVIOUS = HERE.parents[1] / 'kvm-timer-reuse-20261126'
sys.path.insert(0, str(PREVIOUS / 'audit-support'))
from common import digest, identity
from run import verified_evidence


def load(path):
    return json.loads(path.read_text())


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


index = load(HERE / 'evidence-index.json')
for name, expected in index['files'].items():
    assert digest(HERE / name) == expected, name
for name, expected in index['reused_helpers'].items():
    assert digest(PREVIOUS / name) == expected, name
benchmark = module('frozen_budget_benchmark', PREVIOUS / 'audit-controller.py')
assert load(PREVIOUS / 'audit-result.json')['checks']['negative_audit_cases'] == 4
scope = load(HERE / 'source-scope.json')
sources = HERE / 'qualified-sources'
checks = {}
with tempfile.TemporaryDirectory(prefix='trames-budget-audit-') as temporary:
    root = Path(temporary)
    with tarfile.open(HERE / 'raw-evidence.tar.gz') as archive:
        members = archive.getmembers()
        assert len(members) < 10000 and sum(m.size for m in members) < 64 * 1024 * 1024
        for member in members:
            path = Path(member.name)
            assert not path.is_absolute() and '..' not in path.parts
            assert member.isfile() or member.isdir()
        archive.extractall(root)
    for name, expected in index['raw_files'].items():
        assert digest(root / name) == expected, name
    builds = {}
    for variant, commit in [('before', scope['baseline']), ('after', scope['candidate'])]:
        clean = load(root / 'builds' / f'trames-kvm-budget-{variant}-build.json')
        initial = load(root / 'builds' / f'trames-kvm-budget-{variant}-initial-build.json')
        for receipt in (clean, initial):
            assert identity(receipt['fingerprint']) == receipt['fingerprint_sha256']
            assert receipt['sources']['product']['commit'] == commit
            assert receipt['sources']['runtime']['commit'] == scope['cohort_runtime']
        assert all(not v['dirty'] for v in clean['sources'].values())
        assert initial['sources']['product']['dirty'] is True
        assert initial['fingerprint']['binary_sha256'] == clean['fingerprint']['binary_sha256']
        assert initial['sources']['runtime'] == clean['sources']['runtime']
        builds[variant] = clean
    assert builds['before']['sources']['runtime'] == builds['after']['sources']['runtime']
    for key in ('toolchain', 'flags'):
        assert builds['before']['fingerprint'][key] == builds['after']['fingerprint'][key]
    equal = load(root / 'integration/kvm-budget-clean-identity.json')
    assert {r['variant'] for r in equal} == {'before', 'after'}
    for row in equal:
        assert row['initial_sha256'] == row['clean_sha256'] == builds[row['variant']]['fingerprint']['binary_sha256']
    checks['clean_receipts_and_byte_identical_initial_binaries'] = True

    performance = root / 'performance'
    inputs = load(performance / 'inputs.json')
    assert inputs['build'] == builds['after'] and inputs['baseline_build'] == builds['before']
    assert inputs['workers'] == [1, 4] and inputs['repeats'] == 3
    assert inputs['workloads'] == ['cpu', 'getpid', 'pipe', 'futex', 'mmap', 'fork-exec', 'build', 'node']
    assert digest(PREVIOUS / 'audit-controller.py') == inputs['controller_sha256']
    for path, expected in inputs['fixture_sources'].items():
        assert digest(sources / Path(path).name) == expected
    rows = load(performance / 'samples.json')
    assert len(rows) == 320 and sum(r['phase'] == 'sample' for r in rows) == 240
    assert benchmark.audit(performance) == 0
    assert load(performance / 'results.json') == load(HERE / 'performance/results.json')
    checks['raw_executions'] = 320
    checks['measured_samples'] = 240
    checks['unchanged_auditor_negative_controls_reused'] = 4

    for domain, count in [('memory', 77), ('cpu', 15)]:
        data = load(root / domain / 'results.json')
        assert data['passed'] == data['planned'] == len(data['cases']) == count
        assert len({r['id'] for r in data['cases']}) == count
        assert load(root / domain / 'inputs.json')['build']['fingerprint']['binary_sha256'] == builds['after']['fingerprint']['binary_sha256']
        for row in data['cases']:
            assert row['status'] == 'passed' and row['exit_code'] == 0
            folder = root / domain / row['id']
            assert not (folder / 'stderr').read_bytes()
            if domain == 'memory':
                mode = row['id'].removeprefix('native-') if row['id'].startswith('native-') else row['id'].rsplit('-', 2)[0]
                assert (folder / 'stdout').read_text() == mode + ': ok\n'
        checks[domain] = count
    assert set(load(root / 'memory/inputs.json')['load_alignments']) == {4096}
    for domain, fixture, controller in [('memory', 'memory-4k.c', 'validate-memory.py'), ('cpu', 'cpu-extensions.c', 'validate-cpu.py')]:
        data = load(root / domain / 'inputs.json')
        assert digest(sources / fixture) == data['fixture_source_sha256']
        assert digest(sources / controller) == data['controller_sha256']

    clock = module('frozen_budget_clock', root / 'clock/validate-clock.py')
    assert clock.audit(root / 'clock')['passed'] == 5
    checks['clock_consistency'] = 5
    expected = {'binary-stdio', 'literal-env-cwd', 'explicit-rootfs-shell', 'cancel/spin',
                'cancel/syscall-spin', 'cancel/sleep', 'deadline/spin', 'deadline/syscall-spin',
                'deadline/sleep', 'real-pty-size-and-restore'}
    for transport in ('smccc', 'mmio'):
        rows = load(root / f'cli-{transport}/results.json')
        assert len(rows) == 10 and {r['name'] for r in rows} == expected
        for row in rows:
            assert row['passed'] is True
            if row['name'].startswith('deadline/'):
                assert row['code'] == 2 and .15 <= row['seconds'] < 3
                assert row['stderr'] == 'trames: session cancelled by its host\n'
    checks['cli_pty_cancellation_and_host_deadlines'] = 20

    for lane in ['native', 'kvm-smccc-1', 'kvm-smccc-4', 'kvm-mmio-1', 'kvm-mmio-4']:
        directory = root / 'applications' / lane
        plan = load(directory / 'plan.json'); results = load(directory / 'results.json')
        by_id = {r['id']: r for r in results['cases']}
        assert len(plan['cases']) == len(by_id) == 12
        for case in plan['cases']:
            key = identity(dict(case=case, rootfs=plan['bundle_sha256'], harness=plan['harness_sha256']))
            fingerprint = identity(dict(inputs=key, provider=plan['provider'], host={k: v for k, v in plan['host'].items() if k != 'cpu'}))
            assert verified_evidence(directory, by_id[case['id']], case, fingerprint), (lane, case['id'])
    checks['kvm_applications'] = 48
    checks['reused_verified_native_applications'] = 12

    counters = load(root / 'counters/results.json')
    assert counters['passed'] and counters['instrumented'] and not counters['primary_performance']
    assert len(counters['cases']) == 15
    for row in counters['cases']:
        assert row['passed'] and row['exit_code'] == 0
        folder = root / 'counters' / row['id']
        for name, expected_hash in row['logs'].items():
            assert digest(folder / name) == expected_hash
        parsed = {line[2]: float(line[0]) for line in csv.reader((folder / 'counters.csv').read_text().splitlines())
                  if len(line) > 2 and line[2] in row['counters']}
        assert parsed == row['counters']
    for workers in (1, 4):
        for transport in ('smccc', 'mmio'):
            values = {v: next(r['counters'] for r in counters['cases'] if r['mode'] == 'getpid'
                             and r['workers'] == workers and r['transport'] == transport and r['variant'] == v)
                      for v in ('before', 'after')}
            for event in ('syscalls:sys_enter_timer_settime', 'syscalls:sys_enter_rt_sigprocmask'):
                assert values['after'][event] < values['before'][event] / 4
    checks['separate_instrumented_diagnostics'] = 15
    for transport in ('smccc', 'mmio'):
        text = (root / 'logs' / f'trames-kvm-budget-hardware-{transport}.log').read_text()
        assert '11 passed; 0 failed' in text and '8 passed; 0 failed' in text
        assert 'assignment_reuses_budget_and_stops_it_before_the_next_task ... ok' in text
    assert 'private_tick_restarts_host_pipe_io ... ok' in (root / 'logs/trames-kvm-budget-tests.log').read_text()
    checks['real_kvm_library_and_hardware'] = 38

    native = load(root / 'integration/kvm-budget-android-native.json')
    assert native == dict(bind_readonly_root_system_errno=30, bind_readonly_data_errno=0,
                          super_readonly_data_errno=30, independent_tmpfs_errno=0)
    for path in (root / 'integration/kvm-budget-android-linux.log', HERE / 'trames-budget-android-mac.log'):
        text = path.read_text()
        assert '40 passed; 0 failed' in text
        assert 'full_init_profile_keeps_storage_writable_and_derives_primary_zygote ... ok' in text
    assert digest(sources / 'build-busybox.sh') == load(HERE / 'kvm-budget-download-check.json')['source_sha256']
    data = load(HERE / 'kvm-budget-download-check.json')
    assert data['passed'] == data['planned'] == 4 and all(r['passed'] for r in data['cases'])
    assert set(load(HERE / 'busybox-mirror-identity.json').values()) == {'3311dff32e746499f4df0d5df04d7eb396382d7e108bb9250e7b519b837043a4'}
    checks['android_native_semantics_and_linux_mac_tests'] = [40, 40]
    checks['download_integrity_cases'] = 4
    for name in ('runtime-apple', 'linux-apple', 'mac', 'ipad'):
        assert 'Finished `dev` profile' in (HERE / f'trames-budget-{name}-check.log').read_text()
    checks['apple_and_ios_compile'] = True
    result = dict(passed=True, checks=checks, explicit_host_io_kick_qualified=False,
                  qualification='Historical data and original gates only; superseded after explicit host-I/O kick regression', full_product_4k_qualified=False,
                  cohort_runtime=scope['cohort_runtime'], final_runtime=scope['final_runtime'])
    (HERE / 'audit-result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
