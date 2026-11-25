#!/usr/bin/env python3
"""Offline audit of this dated, relocatable presentation evidence bundle."""
from pathlib import Path
import hashlib
import json

HERE = Path(__file__).resolve().parent
PRODUCT = HERE.parents[2]


def read(path):
    return json.loads(path.read_text())


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


manifest = read(HERE / 'files.sha256.json')
actual = {p.relative_to(HERE).as_posix() for p in HERE.rglob('*')
          if p.is_file() and p.name not in ['files.sha256.json', 'audit-result.json']}
assert actual == set(manifest)
for name, expected in manifest.items():
    path = HERE / name
    assert path.resolve().is_relative_to(HERE), name
    assert digest(path) == expected, name

final = HERE / 'trames-wsi-qualification-final'
results = read(final / 'results.json')
expected = {'native-target-lifetimes', 'native-render', 'native-headless'}
for transport in ['smccc', 'mmio']:
    for workers in [1, 4]:
        expected |= {f'headless-{transport}-{workers}', f'kvm-{transport}-{workers}'}
    expected.add(f'kvm-{transport}-4-close')
cases = results['cases']
assert len(cases) == len(expected) == results['planned'] == results['passed'] == 13
assert {case['id'] for case in cases} == expected
assert all(case['status'] == 'passed' for case in cases)
assert results['physical_gpu_qualified'] is False
assert results['performance_qualified'] is False

inputs = read(final / 'inputs.json')
build = read(HERE / 'trames-wsi-qualified-build-final.json')
assert inputs['build'] == build
assert all(not source['dirty'] for source in build['sources'].values())
assert read(HERE / 'unit-artifact.json')['sha256'] == inputs['unit_binary_sha256']
for name, expected_hash in inputs['fixture_sources'].items():
    assert digest(PRODUCT / 'tests/fixtures' / name) == expected_hash, name
assert digest(HERE / 'qualified-controller.py') == inputs['controller_sha256']
doctor = read(final / 'doctor.stdout')
assert doctor['host']['host_arch'] == 'aarch64'
assert doctor['host']['host_page_size'] == 4096
assert doctor['host']['issues'] == []
assert doctor['runtime']['commit'] == build['sources']['runtime']['commit']
assert b'2 passed;' in (final / 'native-targets.log').read_bytes()
assert b'0 failed;' in (final / 'native-targets.log').read_bytes()
for case in cases:
    if case['id'] == 'native-target-lifetimes':
        continue
    folder = final / ('native-case' if case['id'] == 'native-render' else case['id'])
    output = (folder / 'stdout').read_bytes()
    assert case['exit_code'] == 0
    if 'headless' in case['id']:
        assert output == b'headless: ok\n'
    else:
        assert b'wsi-ready frames=16\n' in output
        assert case['frames'] == 16
        assert (folder / 'pixels.stdout').read_bytes() == b'pixels: 320x240 green\n'
        assert digest(folder / 'pixels.stdout') == case['pixels_sha256']
        assert case['user_close'] == case['id'].endswith('-close')
        if not case['user_close']:
            assert b'wsi: ok\n' in output
long = HERE / 'native-long-evidence-path'
long_results = read(long / 'results.json')
assert long_results['passed'] == long_results['planned'] == 3
assert {case['id'] for case in long_results['cases']} == {
    'native-target-lifetimes', 'native-render', 'native-headless'}
assert all(case['status'] == 'passed' for case in long_results['cases'])
assert read(long / 'inputs.json')['controller_sha256'] == digest(PRODUCT / 'scripts/validate-wsi.py')
assert read(long / 'inputs.json')['unit_binary_sha256'] == inputs['unit_binary_sha256']
assert b'2 passed;' in (long / 'native-targets.log').read_bytes()
for architecture in ['arm64', 'amd64']:
    native = HERE / f'ci-native-{architecture}'
    native_results = read(native / 'results.json')
    assert native_results['passed'] == native_results['planned'] == 3
    assert {case['id'] for case in native_results['cases']} == {
        'native-target-lifetimes', 'native-render', 'native-headless'}
    assert all(case['status'] == 'passed' for case in native_results['cases'])
    assert native_results['physical_gpu_qualified'] is False
    assert native_results['performance_qualified'] is False
    native_inputs = read(native / 'inputs.json')
    assert native_inputs['build'] is None
    assert native_inputs['controller_sha256'] == digest(PRODUCT / 'scripts/validate-wsi.py')
    assert native_inputs['fixture_sources'] == inputs['fixture_sources']
    assert b'2 passed;' in (native / 'native-targets.log').read_bytes()
    assert (native / 'native-headless/stdout').read_bytes() == b'headless: ok\n'
    assert (native / 'native-case/pixels.stdout').read_bytes() == b'pixels: 320x240 green\n'
cloud = read(HERE / 'cloud-checks.json')['sources']
for group, count in [('runtime', 2), ('native_wsi', 2), ('mac_pin', 1)]:
    checks = cloud[group]['checks']
    assert len(checks) == count and all(c['conclusion'] == 'success' for c in checks)
build_checks = [c for c in cloud['linux_build']['checks'] if c['name'].startswith('Build/unit')]
assert len(build_checks) == 3 and all(c['conclusion'] == 'success' for c in build_checks)
for name in ['memory-abi', 'guest-musl']:
    assert next(c for c in cloud['ipad_pin']['checks'] if c['name'] == name)['conclusion'] == 'success'
print(json.dumps(dict(verified_files=len(manifest), passed=13, planned=13,
                     native_long_path_passed=3,
                     hosted_native_passed=6,
                     physical_gpu_qualified=False, performance_qualified=False),
                 sort_keys=True))
