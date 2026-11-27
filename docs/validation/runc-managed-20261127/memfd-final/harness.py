#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Original sealed memfd self-reexec on native Linux and named KVM environments."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('desktop', ROOT / 'scripts/validate-desktop.py')
d = importlib.util.module_from_spec(spec)
spec.loader.exec_module(d)


def main(a):
    out = a.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    tools = out / 'tools'
    tools.mkdir()
    shutil.copy2(__file__, out / 'harness.py')
    source = ROOT / 'tests/fixtures/memfd-exec.c'
    shutil.copy2(source, out / 'fixture.c')
    probe = tools / 'memfd-exec'
    argv = ['cc', '-O2', '-Wall', '-Wextra', '-Werror', str(source), '-o', str(probe)]
    subprocess.run(argv, check=True)
    native = subprocess.run([str(probe)], capture_output=True, check=True)
    assert native.stdout == b'MEMFD_EXEC_OK\n'
    (out / 'native.json').write_text(json.dumps(dict(command=argv, binary_sha256=d.digest(probe), stdout=native.stdout.decode(), stderr=native.stderr.decode(), passed=True), indent=2) + '\n')
    if a.native_only:
        return
    assert a.runner and a.receipt, 'KVM requires --runner and --receipt'
    runner = tools / 'runner'
    shutil.copy2(a.runner, runner)
    runner.chmod(0o555)
    receipt = json.loads(a.receipt.read_text())
    assert d.digest(runner) == receipt['fingerprint']['binary_sha256']
    shutil.copy2(a.receipt, out / 'build-receipt.json')
    (out / 'doctor.json').write_bytes(subprocess.check_output([str(runner), 'doctor', '--json', '--require-kvm']))
    root = out / 'rootfs'
    root.mkdir()
    copied = set()
    for f in ['/bin/sh', '/usr/bin/setsid']:
        d.install(f, f, root, copied)
    d.install(probe, '/bin/memfd-exec', root, copied)
    for path in ['tmp', 'root', 'run', 'proc', 'dev', 'etc']:
        (root / path).mkdir(exist_ok=True)
    (out / 'rootfs-inputs.json').write_text(json.dumps({str(p.relative_to(root)):d.digest(p) for p in root.rglob('*') if p.is_file()}, indent=2) + '\n')
    rows = []
    for transport in ['smccc', 'mmio']:
        for cpus in [1, 4]:
            case = out / f'{transport}-{cpus}'
            case.mkdir()
            with tempfile.TemporaryDirectory(prefix='registry-', dir=case) as home:
                env = {k:v for k,v in os.environ.items() if k not in ['DISPLAY', 'WAYLAND_DISPLAY', 'TRAMES_HOME']}
                env.update(TRAMES_HOME=home, TRAMES_KVM_TRANSPORT=transport)
                def run(*args):
                    r = subprocess.run([str(runner), *map(str,args)], env=env, capture_output=True, timeout=60)
                    rows.append(dict(transport=transport, cpus=cpus, args=list(map(str,args)), code=r.returncode, stdout=r.stdout.decode(), stderr=r.stderr.decode()))
                    (out / 'operations.json').write_text(json.dumps(rows, indent=2) + '\n')
                    assert r.returncode == 0, rows[-1]
                    return r.stdout
                try:
                    run('config', '--global', 'set', 'defaults.components.vulkan', 'false', 'defaults.display.windows', 'false')
                    run('env', 'create', 'test', '--rootfs', root, '--cpus', cpus)
                    for _ in range(3):
                        assert run('-e', 'test', 'exec', '--', '/bin/memfd-exec') == b'MEMFD_EXEC_OK\n'
                    run('env', 'restart', 'test')
                    assert run('-e', 'test', 'exec', '--', '/bin/memfd-exec') == b'MEMFD_EXEC_OK\n'
                    run('env', 'stop', 'test')
                finally:
                    subprocess.run([str(runner), 'env', 'stop', 'test'], env=env, capture_output=True, timeout=65)
                    if (Path(home) / 'logs').exists():
                        shutil.copytree(Path(home) / 'logs', case / 'logs')
            print(f'{transport}-{cpus}: sealed memfd exec passed', flush=True)
    (out / 'results.json').write_text(json.dumps(dict(passed=True, runner_sha256=d.digest(runner), operations=len(rows), self_reexecs=16), indent=2) + '\n')


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--runner', type=Path)
    p.add_argument('--receipt', type=Path)
    p.add_argument('--native-only', action='store_true')
    p.add_argument('--output', type=Path, required=True)
    main(p.parse_args())
