#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Paired original workloads on native Linux and KVM; no product instrumentation."""
import argparse
import json
import math
import os
from pathlib import Path
import platform
import random
import select
import shutil
import signal
import statistics
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'runtime/tests/acceptance'))
from common import digest, identity, save
from run import host, receipt

WORK = dict(cpu=50000000, memory=128, getpid=10000, **{'file-4k': 1000},
            pipe=1000, **{'unix-socket': 1000}, futex=1000, mmap=256,
            cow=8, **{'fork-exec': 16}, idle=1000, build=12, node=20000000)


def install(source, root, copied):
    source = Path(source)
    if str(source) in copied:
        return
    copied.add(str(source))
    target = root / str(source).lstrip('/')
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, target)
    result = subprocess.run(['ldd', source], text=True, capture_output=True, timeout=10)
    if result.returncode and 'not a dynamic executable' not in result.stderr:
        raise RuntimeError('ldd failed: ' + str(source) + ': ' + result.stderr)
    for line in result.stdout.splitlines():
        if 'not found' in line:
            raise RuntimeError('missing original library: ' + line)
        for item in line.split():
            if item.startswith('/'):
                install(item, root, copied)


def immutable_files(root):
    return {str(p.relative_to(root)): digest(p) for p in sorted(root.rglob('*'))
            if p.is_file() and p.relative_to(root).parts[0] != 'work'}


def sample_tree(pid):
    """One observer snapshot of unique processes, including children of threads."""
    todo = [pid]; seen = set(); rss = pss = 0; pss_complete = True
    while todo:
        current = todo.pop()
        if current in seen:
            continue
        seen.add(current)
        folder = Path('/proc') / str(current)
        try:
            status = (folder / 'status').read_text()
            rss += next((int(line.split()[1]) * 1024 for line in status.splitlines()
                         if line.startswith('VmRSS:')), 0)
            try:
                rollup = (folder / 'smaps_rollup').read_text()
                pss += next(int(line.split()[1]) * 1024 for line in rollup.splitlines()
                            if line.startswith('Pss:'))
            except (OSError, StopIteration):
                pss_complete = False
            for thread in (folder / 'task').iterdir():
                try:
                    todo.extend(int(item) for item in (thread / 'children').read_text().split())
                except OSError:
                    pass
        except OSError:
            # A short-lived child may exit during this non-atomic snapshot.
            pss_complete = False
    return dict(rss_bytes=rss, pss_bytes=pss if pss_complete else None,
                processes=len(seen))


def measure(argv, environment, folder, timeout):
    folder.mkdir()
    save(folder / 'command.json', dict(argv=[str(x) for x in argv], environment=environment,
                                     timeout_seconds=timeout))
    samples = []; timed_out = False
    with (folder / 'stdout').open('wb') as stdout, (folder / 'stderr').open('wb') as stderr:
        began = time.monotonic_ns()
        process = subprocess.Popen(argv, env=environment, stdin=subprocess.DEVNULL,
                                   stdout=stdout, stderr=stderr, start_new_session=True)
        descriptor = os.pidfd_open(process.pid)
        poll = select.poll(); poll.register(descriptor, select.POLLIN)
        next_sample = began
        try:
            while True:
                child, status, usage = os.wait4(process.pid, os.WNOHANG)
                now = time.monotonic_ns()
                if child:
                    break
                if now - began >= timeout * 1000000000:
                    timed_out = True
                    os.killpg(process.pid, signal.SIGKILL)
                    _, status, usage = os.wait4(process.pid, 0)
                    now = time.monotonic_ns()
                    break
                if now >= next_sample:
                    samples.append(dict(elapsed_ns=now - began, **sample_tree(process.pid)))
                    next_sample = now + 250000000
                delay = min(next_sample - time.monotonic_ns(),
                            began + int(timeout * 1000000000) - time.monotonic_ns())
                poll.poll(max(1, math.ceil(delay / 1000000)))
        finally:
            os.close(descriptor)
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        process.returncode = os.waitstatus_to_exitcode(status)
    save(folder / 'memory-samples.json', samples)
    return dict(exit_code=process.returncode, timed_out=timed_out, wall_ns=now - began,
                user_ns=round(usage.ru_utime * 1e9), system_ns=round(usage.ru_stime * 1e9),
                voluntary_switches=usage.ru_nvcsw, involuntary_switches=usage.ru_nivcsw,
                minor_faults=usage.ru_minflt, major_faults=usage.ru_majflt,
                largest_process_peak_rss_bytes=usage.ru_maxrss * 1024,
                sampled_tree_peak_rss_bytes=max((s['rss_bytes'] for s in samples), default=0),
                sampled_tree_peak_pss_bytes=max((s['pss_bytes'] for s in samples
                                               if s['pss_bytes'] is not None), default=None),
                logs={name: digest(folder / name) for name in ('stdout', 'stderr', 'memory-samples.json', 'command.json')})


def audit(output):
    inputs = json.loads((output / 'inputs.json').read_text())
    rows = json.loads((output / 'samples.json').read_text())
    expected_ids = inputs['planned_ids']
    if len(rows) != len(expected_ids) or {r['id'] for r in rows} != set(expected_ids):
        raise ValueError('incomplete or duplicate performance evidence')
    planned = {case['id']: case for case in inputs['planned_cases']}
    if list(planned) != expected_ids:
        raise ValueError('invalid planned sample schedule')
    expected = {}; groups = {}; failures = []
    # Calibrations are native, correctness checked, and never used as timings.
    for row in rows:
        folder = output / 'cases' / row['id']
        if row != json.loads((folder / 'result.json').read_text()):
            raise ValueError('sample record differs from its evidence: ' + row['id'])
        if any(row.get(key) != value for key, value in planned[row['id']].items()):
            raise ValueError('sample differs from its planned work: ' + row['id'])
        for name, value in row['logs'].items():
            if digest(folder / name) != value:
                raise ValueError('modified performance evidence: ' + row['id'])
        work = row.get('work')
        passed = row['exit_code'] == 0 and not row['timed_out'] and not (folder / 'stderr').read_bytes()
        if passed:
            try:
                parsed = json.loads((folder / 'stdout').read_text())
                passed = isinstance(work, dict) and set(work) == {'mode', 'concurrency', 'tasks', 'iterations', 'checksum', 'work_ns'} and parsed == work and work['mode'] == row['mode'] and work['concurrency'] == row['workers'] and work['iterations'] == row['iterations'] and isinstance(work['work_ns'], int) and work['work_ns'] > 0
            except (ValueError, TypeError, KeyError):
                passed = False
        key = (row['mode'], row['workers'])
        semantic = {k: v for k, v in (work or {}).items() if k != 'work_ns'}
        if row['phase'] == 'calibration':
            if row['lane'] != 'native':
                raise ValueError('non-native calibration')
            if passed:
                expected[key] = semantic
        else:
            passed &= key in expected and semantic == expected.get(key)
        if not passed:
            failures.append(row['id'])
        elif row['phase'] == 'sample':
            groups.setdefault((row['mode'], row['workers'], row['lane']), []).append(row)
    comparisons = []
    for (mode, workers, lane), values in sorted(groups.items()):
        native = groups.get((mode, workers, 'native'), [])
        if len(values) != inputs['repeats'] or len(native) != inputs['repeats']:
            failures.append(f'incomplete distribution: {mode}/{workers}/{lane}')
            continue
        metrics = ('work_ns', 'wall_ns', 'cpu_ns', 'user_ns', 'system_ns', 'voluntary_switches',
                   'involuntary_switches', 'minor_faults', 'major_faults',
                   'largest_process_peak_rss_bytes', 'sampled_tree_peak_rss_bytes',
                   'sampled_tree_peak_pss_bytes')
        def metric(r, name):
            if name == 'cpu_ns':
                return r['user_ns'] + r['system_ns']
            return r['work'][name] if name == 'work_ns' else r[name]
        summary = {}
        for name in metrics:
            series = [metric(r, name) for r in values if metric(r, name) is not None]
            summary[name] = dict(median=statistics.median(series), minimum=min(series), maximum=max(series)) if series else None
        comparisons.append(dict(mode=mode, workers=workers, lane=lane, samples=len(values), metrics=summary,
            work_time_ratio_to_native=statistics.median(r['work']['work_ns'] for r in values) / statistics.median(r['work']['work_ns'] for r in native),
            wall_time_ratio_to_native=statistics.median(r['wall_ns'] for r in values) / statistics.median(r['wall_ns'] for r in native)))
    result = dict(schema_version=1, passed=not failures, failures=failures, comparisons=comparisons,
                  inputs_sha256=digest(output / 'inputs.json'), samples_sha256=digest(output / 'samples.json'),
                  audit_controller_sha256=digest(Path(__file__)),
                  scope='this host/configuration; no universal near-native performance claim')
    save(output / 'results.json', result)
    report = ['# Paired original workload baseline', '',
              'Audit: **' + ('passed' if result['passed'] else 'failed') + '**. ',
              f"{sum(r['phase'] == 'sample' for r in rows)} measured executions; calibration/warmup excluded.", '',
              'Medians are milliseconds. Ratio is KVM/native work time (lower is better).',
              'Wall and CPU include startup; sampled memory may miss short peaks.',
              'This host/configuration only; caches retained; no bare-metal claim.', '',
              '| Workload | Workers | Lane | Work ms | Wall ms | CPU ms | Work/native |',
              '| --- | ---: | --- | ---: | ---: | ---: | ---: |']
    for row in comparisons:
        m = row['metrics']
        report.append(f"| {row['mode']} | {row['workers']} | {row['lane']} | {m['work_ns']['median']/1e6:.3f} | {m['wall_ns']['median']/1e6:.3f} | {m['cpu_ns']['median']/1e6:.3f} | {row['work_time_ratio_to_native']:.3f} |")
    if failures:
        report.extend(['', 'Failed evidence: ' + ', '.join(failures)])
    (output / 'report.md').write_text('\n'.join(report) + '\n')
    print('audit:', 'passed' if result['passed'] else 'failed',
          'measured executions:', sum(r['phase'] == 'sample' for r in rows), flush=True)
    return 0 if result['passed'] else 1


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--runner', type=Path)
    p.add_argument('--receipt', type=Path)
    p.add_argument('--native-only', action='store_true')
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--audit', action='store_true')
    p.add_argument('--workload', choices=list(WORK), action='append')
    p.add_argument('--workers', type=int, choices=(1, 2, 4), nargs='+', default=[1, 2, 4])
    p.add_argument('--transport', choices=('smccc', 'mmio'), nargs='+', default=['smccc', 'mmio'])
    p.add_argument('--repeats', type=int, default=5)
    p.add_argument('--scale', type=float, default=1)
    p.add_argument('--affinity', help='ordered, comma-separated allowed host CPUs (default: last four)')
    p.add_argument('--timeout', type=float, default=120)
    p.add_argument('--seed', type=int, default=4096)
    a = p.parse_args(); out = a.output.resolve()
    if a.audit:
        return audit(out)
    if platform.system() != 'Linux' or os.sysconf('SC_PAGE_SIZE') != 4096 or not hasattr(os, 'pidfd_open'):
        p.error('requires real 4KiB Linux with pidfd exit notification')
    if a.repeats < 1 or a.scale <= 0 or a.timeout <= 0:
        p.error('positive repeats, scale and timeout required')
    if not a.native_only and not (a.runner and a.receipt):
        p.error('KVM lanes require --runner and --receipt')
    allowed = sorted(os.sched_getaffinity(0))
    cpus = [int(c) for c in a.affinity.split(',')] if a.affinity else allowed[-4:]
    if len(set(cpus)) != len(cpus) or not set(cpus) <= set(allowed) or max(a.workers) > len(cpus):
        p.error('affinity must supply enough distinct allowed CPUs for every worker count')
    modes = a.workload or list(WORK)
    if len(set(modes)) != len(modes) or len(set(a.workers)) != len(a.workers):
        p.error('duplicate workloads or worker counts')
    out.mkdir(parents=True, exist_ok=False); root = out / 'rootfs'
    (root / 'test').mkdir(parents=True); (root / 'work').mkdir()
    (root / 'tmp').mkdir(mode=0o1777)
    binary = root / 'test/performance'; source = ROOT / 'tests/fixtures/performance.c'
    command = [os.environ.get('CC', 'cc'), '-O2', '-static', '-pthread', '-Wall', '-Wextra', '-Werror',
               '-Wl,-z,max-page-size=4096', '-Wl,-z,common-page-size=4096', str(source), '-o', str(binary)]
    with (out / 'fixture.build.log').open('wb') as log:
        subprocess.run(command, check=True, stdout=log, stderr=subprocess.STDOUT)
    headers = subprocess.check_output(['readelf', '--wide', '-l', binary], text=True)
    (out / 'elf-segments.txt').write_text(headers)
    loads = [line.split() for line in headers.splitlines() if line.strip().startswith('LOAD ')]
    if not loads or any(int(line[-1], 16) != 4096 for line in loads):
        raise ValueError('performance ELF requires 4096-byte LOAD alignment')
    versions = {}; copied = set()
    if 'node' in modes:
        install('/usr/bin/node', root, copied)
        # Debian/Ubuntu externalize these Node builtins; ldd does not list JS.
        for package in ('acorn', 'acorn-walk', 'cjs-module-lexer', 'undici'):
            shared = Path('/usr/share/nodejs') / package
            if shared.is_dir():
                shutil.copytree(shared, root / str(shared).lstrip('/'))
        shutil.copy2(ROOT / 'tests/fixtures/performance-node.js', root / 'test/performance-node.js')
        versions['node'] = subprocess.check_output(['/usr/bin/node', '--version'], text=True)
    if 'build' in modes:
        for program in ['/usr/bin/make', '/bin/sh', '/usr/bin/cc', '/usr/bin/as',
                        subprocess.check_output(['/usr/bin/cc', '-print-prog-name=cc1'], text=True).strip()]:
            install(program, root, copied)
        for program in ['cc', 'make', 'as']:
            versions[program] = subprocess.check_output(['/usr/bin/' + program, '--version'], text=True)
    build = receipt(a.receipt.resolve(), a.runner.resolve()) if not a.native_only else None
    runner = a.runner.resolve() if build else None
    if runner:
        doctor = subprocess.run([runner, 'doctor', '--json', '--require-kvm'], capture_output=True, timeout=15)
        (out / 'doctor.stdout').write_bytes(doctor.stdout); (out / 'doctor.stderr').write_bytes(doctor.stderr)
        if doctor.returncode:
            raise RuntimeError('real KVM preflight failed')
    immutable = immutable_files(root)
    lanes = ['native'] + ([] if a.native_only else list(a.transport))
    cases = []
    randomizer = random.Random(a.seed)
    for mode in modes:
        count = max(1, round(WORK[mode] * a.scale))
        for workers in a.workers:
            phases = [('calibration', 0, 'native')]
            phases += [('warmup', 0, lane) for lane in lanes if lane != 'native']
            for repeat in range(a.repeats):
                order = lanes.copy(); randomizer.shuffle(order)
                phases += [('sample', repeat, lane) for lane in order]
            for phase, repeat, lane in phases:
                cases.append(dict(id=f'{mode}-{workers}-{phase}-{repeat}-{lane}', mode=mode,
                                  workers=workers, iterations=count, phase=phase, repeat=repeat, lane=lane))
    environment = dict(PATH='/usr/bin:/bin', HOME='/tmp', LANG='C', LC_ALL='C', TZ='UTC')
    save(out / 'inputs.json', dict(schema_version=1, host=host(), build=build, lanes=lanes,
        workloads=modes, workers=a.workers, repeats=a.repeats, scale=a.scale, seed=a.seed,
        affinity=cpus, allowed_cpus=allowed, compiler_command=command,
        compiler_version=subprocess.check_output([command[0], '--version'], text=True), versions=versions,
        fixture_sources={str(s.relative_to(ROOT)): digest(s) for s in [source, ROOT / 'tests/fixtures/performance-node.js']},
        controller_sha256=digest(Path(__file__)), immutable_files=immutable,
        immutable_files_sha256=identity(immutable), planned_ids=[case['id'] for case in cases], planned_cases=cases,
        measurement=dict(wall='pidfd exit notification; includes process/session startup',
            work='original workload monotonic clock; setup/prefault outside timed region',
            cpu='wait4 user/system including descendants reaped by their parents',
            memory='250ms process-tree RSS/PSS snapshots; may miss short peaks; shared RSS double-counts',
            peak_rss='wait4 largest single-process peak, not summed simultaneous tree peak',
            cache='separate process warmups, host page cache retained; no cold-cache claim',
            observer='external controller only; no product tracing/counters/extra service threads',
            unavailable=['KVM exits and ioctl counts (separate traced diagnostic required)', 'physical-host CPU/GPU throughput'])) )
    spare = set(allowed) - set(cpus)
    if spare:
        os.sched_setaffinity(0, {min(spare)})
    (out / 'cases').mkdir(); rows = []
    try:
        for case in cases:
            work = root / 'work' / case['id']; work.mkdir()
            if case['mode'] == 'node':
                args = ['/usr/bin/node', '/test/performance-node.js', str(case['workers']), str(case['iterations'])]
                native_args = ['/usr/bin/node', str(root / 'test/performance-node.js'), *args[2:]]
            else:
                args = ['/test/performance', case['mode'], str(case['iterations']), str(case['workers']), '/work/' + case['id']]
                native_args = [str(binary), *args[1:-1], str(work)]
            env = environment.copy()
            if case['lane'] == 'native':
                invocation = native_args
            else:
                env['TRAMES_KVM_TRANSPORT'] = case['lane']
                invocation = [runner, 'run', '--cpus', str(case['workers']), '--rootfs', root,
                              '--timeout-ms', str(round(a.timeout * 1000)), '--', *args]
            invocation = ['/usr/bin/taskset', '-c', ','.join(str(c) for c in cpus[:case['workers']]), *invocation]
            folder = out / 'cases' / case['id']
            measured = measure(invocation, env, folder, a.timeout + 5)
            try:
                value = json.loads((folder / 'stdout').read_text())
            except (ValueError, OSError):
                value = None
            row = dict(**case, **measured, work=value)
            save(folder / 'result.json', row); rows.append(row); save(out / 'samples.json', rows)
            print(case['id'], 'exit=' + str(row['exit_code']), 'wall_ms=' + str(round(row['wall_ns'] / 1e6, 3)), flush=True)
            shutil.rmtree(work)
    finally:
        os.sched_setaffinity(0, allowed)
    if immutable_files(root) != immutable:
        raise ValueError('original application/library bytes changed during measurement')
    return audit(out)


if __name__ == '__main__':
    raise SystemExit(main())
