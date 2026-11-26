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
PREVIOUS = HERE.parent / 'kvm-timer-reuse-20261126'
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
initial = module('historical_budget_audit', HERE / 'initial/audit.py')
assert load(HERE / 'initial/audit-result.json')['explicit_host_io_kick_qualified'] is False
scope = load(HERE / 'source-scope.json')
sources = HERE / 'qualified-sources'
checks = {}
for name, value in scope['qualified_sources'].items():
    assert digest(sources / name) == value['sha256'], name
cloud = load(HERE / 'cloud-checks.json')
for key, expected_count in [('linux', 6), ('runtime', 3), ('mac', 2)]:
    rows = cloud[key]['rows']
    assert len(rows) == expected_count and all(r['conclusion'] == 'success' for r in rows)
assert cloud['linux']['commit'] == scope['candidate']
assert cloud['runtime']['commit'] == scope['cohort_runtime']
assert next(r for r in cloud['ipad']['rows'] if r['name'] == 'runtime')['conclusion'] == 'failure'
ipad_failures = load(HERE / 'ipad-qemu-failures.json')
assert ipad_failures['failed'] == len(set(ipad_failures['failures'])) == 18
checks['linux_cloud_and_shared_runtime_mac_ci'] = True
checks['ipad_broad_oracle_unresolved_failures'] = 18
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
    for variant, commit, runtime_commit in [('before', scope['baseline'], scope['baseline_runtime']),
                                           ('final', scope['candidate'], scope['cohort_runtime'])]:
        value = load(root / 'builds' / f'trames-kvm-budget-{variant}-build.json')
        assert identity(value['fingerprint']) == value['fingerprint_sha256']
        assert value['sources']['product']['commit'] == commit
        assert value['sources']['runtime']['commit'] == runtime_commit
        assert all(not v['dirty'] for v in value['sources'].values())
        builds['after' if variant == 'final' else 'before'] = value
    # The only Runtime delta is ignored test code; the active Android implementation is unchanged.
    old_android = (sources / 'android-before-test-fix.rs').read_bytes()
    new_android = (sources / 'android-test-fix.rs').read_bytes()
    assert old_android.split(b'#[cfg(test)]')[0] == new_android.split(b'#[cfg(test)]')[0]
    import hashlib
    assert hashlib.sha256(old_android.split(b'#[cfg(test)]')[0]).hexdigest() == scope['android_production_prefix_sha256']
    assert scope['android_changed_files'] == ['crates/trames-distro/src/android.rs']
    for key in ('toolchain', 'flags'):
        assert builds['before']['fingerprint'][key] == builds['after']['fingerprint'][key]
    negative = (root / 'logs/trames-kvm-budget-kick-negative.log').read_text()
    assert 'explicit_kick_interrupts_host_pipe_io ... FAILED' in negative
    assert 'left: 1' in negative and 'right: -1' in negative
    original_deadline = (HERE / 'initial/qualified-sources/deadline.rs').read_bytes()
    assert original_deadline.split(b'#[cfg(test)]')[0] == (HERE / 'negative-deadline.rs').read_bytes().split(b'#[cfg(test)]')[0]
    checks['initial_explicit_host_io_kick_regression_reproduced'] = True
    checks['production_runtime_equivalence'] = True

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
    checks['final_raw_executions'] = 320
    checks['initial_raw_executions'] = 320
    checks['final_measured_samples'] = 240
    checks['initial_measured_samples'] = 240
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
    assert digest(root / 'controllers/kvm-budget-final-counters.py') == load(root / 'counters/inputs.json')['controller_sha256']
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
        text = (root / 'logs' / f'trames-kvm-budget-final-hardware-{transport}.log').read_text()
        assert '11 passed; 0 failed' in text and '8 passed; 0 failed' in text
        assert 'assignment_reuses_budget_and_stops_it_before_the_next_task ... ok' in text
    assert 'private_tick_restarts_host_pipe_io ... ok' in (root / 'logs/trames-kvm-budget-final-tests.log').read_text()
    tests = (root / 'logs/trames-kvm-budget-final-tests.log').read_text()
    assert 'explicit_kick_interrupts_host_pipe_io ... ok' in tests
    assert 'assignment_restores_both_original_signal_masks ... ok' in tests
    checks['real_kvm_library_and_hardware'] = 38
    checks['host_tick_restart_and_explicit_kick_interrupt_are_distinct'] = True

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
    assert 'Finished `dev` profile' in (HERE / 'trames-budget-final-apple-check.log').read_text()
    checks['apple_and_ios_compile'] = True
    cleanup = load(root / 'integration/kvm-budget-final-rootfs-cleanup.json')
    assert cleanup['retained_master_unchanged'] and cleanup['retained_master_files'] == 169
    assert len(cleanup['removed_only_completed_task_rootfs']) == 5
    checks['temporary_rootfs_cleanup_verified'] = True
    result = dict(passed=True, qualification='Scoped final KVM evidence and archive integrity; broad iPad oracle remains unqualified', checks=checks, full_product_4k_qualified=False,
                  cohort_runtime=scope['cohort_runtime'], final_runtime=scope['final_runtime'])
    (HERE / 'audit-result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
