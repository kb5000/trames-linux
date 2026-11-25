#!/usr/bin/env python3
"""Verify frozen KVM timer evidence; no VM, images or benchmark execution."""
from pathlib import Path
import contextlib
import importlib.util
import io
import json
import sys
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / 'audit-support'))
from common import digest, identity
from run import verified_evidence

spec = importlib.util.spec_from_file_location('frozen_benchmark', HERE / 'audit-controller.py')
benchmark = importlib.util.module_from_spec(spec)
spec.loader.exec_module(benchmark)


def load(path):
    return json.loads(path.read_text())


def save(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def negatives(output):
    """Mutate temporary evidence, keeping raw and semantic checks independent."""
    original = (output / 'samples.json').read_bytes()
    rows = load(output / 'samples.json')
    row = next(r for r in rows if r['phase'] == 'sample' and r['lane'] == 'candidate-smccc')
    folder = output / 'cases' / row['id']
    raw = (folder / 'stdout').read_bytes()
    record = (folder / 'result.json').read_bytes()
    results = []

    def check(name):
        try:
            with contextlib.redirect_stdout(io.StringIO()):
                rejected = benchmark.audit(output) != 0
        except (ValueError, AssertionError):
            rejected = True
        assert rejected, name
        results.append(dict(name=name, rejected=True))

    try:
        (folder / 'stdout').write_bytes(raw + b'changed\n')
        check('changed raw output')
        (folder / 'stdout').write_bytes(raw)
        save(output / 'samples.json', rows[:-1])
        check('missing planned sample')
        save(output / 'samples.json', rows)
        row['workers'] = 2
        save(folder / 'result.json', row)
        save(output / 'samples.json', rows)
        check('self-consistent sample differs from planned worker count')
        row['workers'] = next(r['workers'] for r in json.loads(original) if r['id'] == row['id'])
        row['work']['checksum'] += 1
        save(folder / 'stdout', row['work'])
        row['logs']['stdout'] = digest(folder / 'stdout')
        save(folder / 'result.json', row)
        save(output / 'samples.json', rows)
        check('self-consistent output differs from native checksum')
    finally:
        (folder / 'stdout').write_bytes(raw)
        (folder / 'result.json').write_bytes(record)
        (output / 'samples.json').write_bytes(original)
    assert benchmark.audit(output) == 0
    return results


index = load(HERE / 'evidence-index.json')
for name, expected in index['files'].items():
    assert digest(HERE / name) == expected, name
checks = {}
with tempfile.TemporaryDirectory(prefix='trames-timer-audit-') as temporary:
    root = Path(temporary)
    with tarfile.open(HERE / 'raw-evidence.tar.gz', 'r:gz') as archive:
        members = archive.getmembers()
        assert len(members) < 15000 and sum(m.size for m in members) < 48 * 1024 * 1024
        for member in members:
            path = Path(member.name)
            assert not path.is_absolute() and '..' not in path.parts
            assert member.isfile() or member.isdir()
        archive.extractall(root)
    builds = {}
    for name, commit in [('before', 'cadd2ac6c3ea2b1da3fd8f17eebf6907b44039c0'),
                         ('after', 'd9b4803ede684e2355ff3b079ee67d541ac4098e'),
                         ('fuse', '88b6d8505ad51a5a0d96fb10ba4e6666367eea0c')]:
        build = load(root / 'builds-and-logs' / f'trames-kvm-timer-{name}-build.json')
        assert identity(build['fingerprint']) == build['fingerprint_sha256']
        assert build['sources']['product']['commit'] == commit
        assert build['sources']['runtime']['commit'] == 'a37a84d4b3c66f21ddb581f8f4b0427f50793e0d'
        assert all(not source['dirty'] for source in build['sources'].values())
        builds[name] = build
    assert builds['before']['fingerprint']['toolchain'] == builds['after']['fingerprint']['toolchain']
    assert builds['before']['fingerprint']['flags'] == builds['after']['fingerprint']['flags']
    sources = HERE / 'qualified-sources'
    before_inputs = load(root / 'before-performance/inputs.json')
    after_inputs = load(root / 'after-performance/inputs.json')
    for key in ['fixture_sources', 'immutable_files_sha256', 'versions', 'affinity']:
        assert before_inputs[key] == after_inputs[key], key
    for name, count, build in [('before-performance', 108, 'before'),
                               ('after-performance', 108, 'after'), ('paired-micro', 150, 'after')]:
        directory = root / name
        inputs = load(directory / 'inputs.json')
        assert inputs['build'] == builds[build] and inputs['host']['page_size'] == 4096
        if name == 'paired-micro':
            assert inputs['baseline_build'] == builds['before']
        controller = sources / ('paired-benchmark.py' if name == 'paired-micro' else 'macro-benchmark.py')
        assert digest(controller) == inputs['controller_sha256']
        for path, value in inputs['fixture_sources'].items():
            assert digest(sources / Path(path).name) == value
        assert benchmark.audit(directory) == 0
        assert sum(r['phase'] == 'sample' for r in load(directory / 'samples.json')) == count
        assert load(directory / 'results.json') == load(HERE / name / 'results.json')
        checks[name + '_measured'] = count
    negative = negatives(root / 'paired-micro')
    checks['negative_audit_cases'] = len(negative)

    modes = ('geometry', 'protection', 'holes', 'owners', 'file-offsets', 'truncate',
             'discard-remap', 'cow', 'threads', 'retire', 'exec')
    expected = {f'native-{m}': m for m in modes}
    expected.update({f'{m}-{t}-{w}': m for m in modes for t in ['smccc', 'mmio'] for w in [1, 2, 4]})
    memory = load(root / 'memory/results.json')
    assert memory['passed'] == memory['planned'] == len(memory['cases']) == 77
    assert {r['id'] for r in memory['cases']} == set(expected)
    for row in memory['cases']:
        assert row['status'] == 'passed' and row['exit_code'] == 0
        folder = root / 'memory' / row['id']
        assert (folder / 'stdout').read_bytes() == (expected[row['id']] + ': ok\n').encode()
        assert not (folder / 'stderr').read_bytes()
    inputs = load(root / 'memory/inputs.json')
    assert inputs['build'] == builds['after'] and set(inputs['load_alignments']) == {4096}
    assert digest(sources / 'memory-4k.c') == inputs['fixture_source_sha256']
    assert digest(sources / 'validate-memory.py') == inputs['controller_sha256']
    checks['core_memory'] = 77
    cpu = load(root / 'cpu/results.json')
    assert cpu['passed'] == cpu['planned'] == len(cpu['cases']) == 15
    for row in cpu['cases']:
        assert row['status'] == 'passed' and row['exit_code'] == 0
        mode = row['id'].split('-')[1 if row['id'].startswith('native-') else 0]
        assert (root / 'cpu' / row['id'] / 'stdout').read_bytes() == (root / 'cpu' / ('native-' + mode) / 'stdout').read_bytes()
    assert load(root / 'cpu/inputs.json')['build'] == builds['after']
    inputs = load(root / 'cpu/inputs.json')
    assert digest(sources / 'cpu-extensions.c') == inputs['fixture_source_sha256']
    assert digest(sources / 'validate-cpu.py') == inputs['controller_sha256']
    checks['cpu_state_ptrace'] = 15

    for label in ['native', 'kvm-smccc-1', 'kvm-smccc-4', 'kvm-mmio-1', 'kvm-mmio-4']:
        directory = root / ('applications-native' if label == 'native' else 'applications/' + label)
        plan = load(directory / 'plan.json')
        rows = load(directory / 'results.json')['cases']
        assert len(rows) == len(plan['cases']) == 12
        by_id = {r['id']: r for r in rows}
        assert set(by_id) == {case['id'] for case in plan['cases']}
        for case in plan['cases']:
            inputs = identity(dict(case=case, rootfs=plan['bundle_sha256'], harness=plan['harness_sha256']))
            fingerprint = identity(dict(inputs=inputs, provider=plan['provider'], host={k: v for k, v in plan['host'].items() if k != 'cpu'}))
            assert verified_evidence(directory, by_id[case['id']], case, fingerprint), (label, case['id'])
    checks['application_kvm_cells'] = 48
    checks['reused_native_application_cells'] = 12
    for name in ['cli-smccc', 'cli-mmio', 'fuse-cli-smccc', 'fuse-cli-mmio']:
        rows = load(root / name / 'results.json')
        assert len(rows) == 6 and all(r['passed'] for r in rows)
    checks['cli_before_and_after_device_fix'] = 24

    diagnostic = load(root / 'counters/results.json')
    assert diagnostic['passed'] and diagnostic['instrumented'] and not diagnostic['primary_performance']
    assert len(diagnostic['cases']) == 15
    for row in diagnostic['cases']:
        assert row['passed'] and row['exit_code'] == 0
        for name, value in row['logs'].items():
            assert digest(root / 'counters' / row['id'] / name) == value
    by_id = {r['id']: r for r in diagnostic['cases']}
    for workers in [1, 4]:
        for transport in ['smccc', 'mmio']:
            before = by_id[f'getpid-{workers}-before-{transport}']['counters']
            after = by_id[f'getpid-{workers}-after-{transport}']['counters']
            for event in ['timer_create', 'timer_delete']:
                key = 'syscalls:sys_enter_' + event
                assert before[key] > 10000 * workers and after[key] == workers
    checks['separate_counter_diagnostics'] = 15

    fuse = load(root / 'fuse-final/results.json')
    assert fuse['planned'] == fuse['passed'] == len(fuse['cases']) == 12
    assert fuse['checks_passed'] == 144
    assert {r['id'] for r in fuse['cases']} == {f'{p}-{t}-{w}' for p in ['lowlevel', 'writeback', 'highlevel'] for t in ['smccc', 'mmio'] for w in [1, 4]}
    assert load(root / 'fuse-final/inputs.json')['build'] == builds['fuse']
    for row in fuse['cases']:
        assert row['passed'] and row['exit_code'] == 0 and all(row['checks'].values())
        folder = root / 'fuse-final' / row['id']
        for name, value in row['logs'].items():
            assert digest(folder / name) == value
        text = (folder / 'stdout').read_text()
        assert all(text.count('CASE ' + name + ' PASS\n') == 1 for name in row['checks'])
        assert (folder / 'fuse-daemon.exit').read_text().strip() == '0'
        if row['id'].startswith('writeback-'):
            assert 'activating writeback' in (folder / 'fuse-daemon.stderr').read_text()
    checks['stock_fuse_functional_and_shutdown'] = 144
    assert load(root / 'fuse-initial/results.json')['passed'] == 0
    checks['initial_fuse_failure_retained'] = True

result = dict(passed=True, checks=checks, negative_tests=negative,
              raw_archive_sha256=digest(HERE / 'raw-evidence.tar.gz'), full_product_4k_qualified=False)
save(HERE / 'audit-result.json', result)
print(json.dumps(result, indent=2))
