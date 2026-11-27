#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Named DNS and native systemd user-service acceptance in disposable registries.

Requires real KVM and an existing systemd user manager. Installs only uniquely
UUID-named test units, removes them on exit, and never restarts the user's target.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import select
import shutil
import signal
import socket
import struct
import subprocess
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('desktop', ROOT / 'scripts/validate-desktop.py')
d = importlib.util.module_from_spec(spec)
spec.loader.exec_module(d)


def main(a):
    out = a.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    tools = out / 'tools'
    tools.mkdir()
    runner = tools / 'runner'
    shutil.copy2(a.runner, runner)
    runner.chmod(0o555)
    receipt = json.loads(a.receipt.read_text())
    assert receipt['fingerprint']['binary_sha256'] == d.digest(runner)
    shutil.copy2(a.receipt, out / 'build-receipt.json')
    shutil.copy2(__file__, out / 'harness.py')
    (out / 'doctor.json').write_bytes(subprocess.check_output([runner, 'doctor', '--json', '--require-kvm']))
    env = dict(os.environ)
    for key in ['DISPLAY', 'WAYLAND_DISPLAY', 'TRAMES_HOME']:
        env.pop(key, None)
    subprocess.run(['systemctl', '--user', 'show-environment'], env=env, check=True, stdout=subprocess.DEVNULL)
    fixture = tools / 'network-service'
    subprocess.run(['cc', '-O2', '-Wall', '-Wextra', '-Werror', '-static', ROOT / 'tests/fixtures/network-service.c', '-o', fixture], check=True)
    shutil.copy2(ROOT / 'tests/fixtures/network-service.c', out / 'fixture.c')
    source = out / 'rootfs'
    source.mkdir()
    copied = set()
    for file in ['/bin/sh', '/usr/bin/setsid', '/usr/bin/curl', '/bin/cat', '/bin/sleep']:
        d.install(file, file, source, copied)
    shutil.copy2(fixture, source / 'bin/netprobe')
    for path in ['etc', 'tmp', 'run', 'root', 'dev', 'proc']:
        (source / path).mkdir(exist_ok=True)
    (out / 'rootfs-inputs.json').write_text(json.dumps({str(p.relative_to(source)): d.digest(p) for p in source.rglob('*') if p.is_file()}, indent=2) + '\n')
    if a.systemd_rootfs:
        (out/'systemd-input.json').write_text(json.dumps(dict(path=str(a.systemd_rootfs), archive_sha256=d.digest(a.systemd_rootfs) if a.systemd_rootfs.is_file() else None), indent=2)+'\n')
    rows = []

    def command(argv, home=None, ok=True, data=None, timeout=100):
        began = time.monotonic()
        r = subprocess.run(list(map(str, argv)), env=env | ({'TRAMES_HOME': str(home)} if home else {}), input=data, capture_output=True, timeout=timeout)
        row = dict(argv=list(map(str, argv)), code=r.returncode, stdout=r.stdout.decode(errors='replace'), stderr=r.stderr.decode(errors='replace'), seconds=time.monotonic()-began)
        rows.append(row)
        (out / 'operations.json').write_text(json.dumps(rows, indent=2) + '\n')
        assert (r.returncode == 0) == ok, row
        return r.stdout

    def op(home, *args, **kw):
        return command([runner, *args], home=home, **kw)

    def execute(home, name, *args, **kw):
        return op(home, '-e', name, 'exec', '--', *args, **kw)

    def inspect(home, name):
        return json.loads(op(home, 'env', 'inspect', name, '--json'))

    def main_pid(unit):
        return int(command(['systemctl', '--user', 'show', unit, '-p', 'MainPID', '--value']).strip() or 0)

    def free_port():
        with socket.socket() as s:
            s.bind(('127.0.0.1', 0))
            return s.getsockname()[1]

    def wait_ready(home, name, old_pid=0):
        deadline = time.monotonic()+30
        while True:
            state = inspect(home, name)
            pid = main_pid(state['service']['unit'])
            if state['state'] == 'running' and pid and pid != old_pid:
                return pid
            assert time.monotonic() < deadline, state
            time.sleep(.1)

    def peer_pid(home, name):
        directory = Path(op(home, 'env', 'path', name).decode().strip())
        endpoint = Path('/tmp') / f'trames-{os.getuid()}' / (hashlib.sha256(str(directory).encode()).hexdigest()[:32]+'.exec')
        with socket.socket(socket.AF_UNIX) as peer:
            peer.connect(str(endpoint))
            pid, uid, _ = struct.unpack('3i', peer.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
            assert uid == os.getuid()
            return pid

    for transport in ['smccc', 'mmio']:
        for cpus in [1, 4]:
            label = f'{transport}-{cpus}'
            if a.case and label not in a.case:
                continue
            case = out / label
            case.mkdir()
            env['TRAMES_KVM_TRANSPORT'] = transport
            children = []
            dropins = []
            # Special characters exercise actual systemd parsing, not a shell.
            with tempfile.TemporaryDirectory(prefix='registry space $data%-', dir=case) as temp:
                home = Path(temp)
                try:
                    op(home, 'config', '--global', 'set', 'defaults.components.vulkan', 'false', 'defaults.display.windows', 'false', 'defaults.resources.cpus', cpus)
                    for network in ['lan', 'other']:
                        op(home, 'network', 'create', network, '--internal')
                    for name, network in [('web', 'lan'), ('db', 'lan'), ('isolated', 'other')]:
                        op(home, 'env', 'create', name, '--rootfs', source)
                        op(home, '-e', name, 'config', 'set', 'network.mode', 'bridge', 'network.bridge', network)
                    port = free_port()
                    op(home, 'env', 'publish', 'db', port, '41000')
                    def select_transport(name):
                        state = inspect(home, name)
                        parent = Path(state['service']['path']+'.d')
                        parent.mkdir(parents=True, exist_ok=False)
                        path = parent/'99-trames-acceptance.conf'
                        path.write_text(f'[Service]\nEnvironment=TRAMES_KVM_TRANSPORT={transport}\n')
                        dropins.append(path)
                    select_transport('db')
                    op(home, 'env', 'enable', 'db', '--now')
                    initial = inspect(home, 'db')
                    unit = initial['service']['unit']
                    assert initial['service']['installed'] and initial['service']['enabled']
                    assert main_pid(unit) == peer_pid(home, 'db')
                    assert f'TRAMES_KVM_TRANSPORT={transport}'.encode() in Path(f'/proc/{main_pid(unit)}/environ').read_bytes().split(b'\0')
                    command(['systemd-analyze', '--user', 'verify', initial['service']['path']])
                    assert unit.encode() in command(['systemctl', '--user', 'list-dependencies', 'default.target', '--plain'])
                    op(home, 'env', 'start', 'web')
                    op(home, 'env', 'start', 'isolated')
                    networks = json.loads(op(home, 'network', 'ls', '--json'))
                    id_ = next(n for n in networks if n['name'] == 'lan')['members'][initial['uuid']]

                    def resolve(name='db', exists=True):
                        for mode, expected in [('resolve', f'10.0.0.{id_}'), ('resolve6', f'fd00::{id_:x}')]:
                            answer = execute(home, 'web', '/bin/netprobe', mode, name+'.trames', ok=exists)
                            if exists:
                                assert answer.strip().decode() == expected, (name, mode, answer)

                    def http(name):
                        path = case / f'http-{len(children)}.stderr'
                        err = path.open('wb')
                        p = subprocess.Popen([str(runner), '-e', name, 'exec', '--', '/bin/netprobe', 'http', 'tcp', '0.0.0.0', '41000', '2'], env=env | {'TRAMES_HOME': str(home)}, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=err)
                        err.close()
                        children.append(p)
                        assert select.select([p.stdout], [], [], 15)[0] and p.stdout.readline() == b'READY\n', path.read_bytes()
                        assert execute(home, 'web', '/usr/bin/curl', '--noproxy', '*', '-fsS', '--max-time', '10', f'http://{name}.trames:41000/') == b'TRAMES_HTTP_OK\n'
                        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
                        assert opener.open(f'http://127.0.0.1:{port}/', timeout=10).read() == b'TRAMES_HTTP_OK\n'
                        p.wait(timeout=10)
                        assert p.returncode == 0, path.read_bytes()

                    resolve()
                    resolve('DB')
                    resolve('db.env')
                    resolve(f'env{id_}')
                    execute(home, 'isolated', '/bin/netprobe', 'resolve', 'db.trames', ok=False)
                    http('db')
                    op(home, 'env', 'logs', 'db')
                    execute(home, 'db', '/bin/sh', '-ec', 'echo durable > /root/value')
                    old_pid = main_pid(unit)
                    command(['systemctl', '--user', 'kill', '--kill-whom=main', '--signal=KILL', unit])
                    pid = wait_ready(home, 'db', old_pid)
                    assert pid == peer_pid(home, 'db')
                    assert execute(home, 'db', '/bin/cat', '/root/value') == b'durable\n'
                    resolve()
                    http('db')
                    op(home, 'env', 'restart', 'db')
                    assert main_pid(unit) != pid
                    resolve()
                    op(home, 'env', 'remove', 'db', '--yes', ok=False)
                    op(home, 'env', 'stop', 'db')
                    resolve(exists=False)
                    op(home, 'env', 'rename', 'db', 'database')
                    assert inspect(home, 'database')['service']['unit'] == unit
                    op(home, 'env', 'start', 'database')
                    resolve('db', exists=False)
                    resolve('database')
                    http('database')
                    op(home, 'env', 'create', 'Database', '--rootfs', source)
                    op(home, '-e', 'Database', 'config', 'set', 'network.mode', 'bridge', 'network.bridge', 'lan')
                    op(home, 'env', 'start', 'Database', ok=False)
                    assert inspect(home, 'Database')['state'] == 'stopped'
                    # A killed unmanaged peer leaves socket files but must disappear from DNS.
                    op(home, 'env', 'create', 'victim', '--rootfs', source)
                    op(home, '-e', 'victim', 'config', 'set', 'network.mode', 'bridge', 'network.bridge', 'lan')
                    op(home, 'env', 'start', 'victim')
                    execute(home, 'web', '/bin/netprobe', 'resolve', 'victim.trames')
                    os.kill(peer_pid(home, 'victim'), signal.SIGKILL)
                    deadline = time.monotonic()+10
                    while inspect(home, 'victim')['state'] != 'stopped':
                        assert time.monotonic() < deadline
                        time.sleep(.02)
                    execute(home, 'web', '/bin/netprobe', 'resolve', 'victim.trames', ok=False)
                    op(home, 'env', 'stop', 'database')
                    op(home, 'env', 'clone', 'database', 'copy')
                    archive = home / 'backup.tar.gz'
                    op(home, 'env', 'backup', 'database', archive)
                    op(home, 'env', 'restore', 'recovered', archive)
                    for name in ['copy', 'recovered']:
                        assert inspect(home, name)['uuid'] != initial['uuid']
                        assert not inspect(home, name)['service']['installed']
                        op(home, 'env', 'unpublish', name, port)
                        op(home, 'env', 'start', name)
                        assert execute(home, name, '/bin/cat', '/root/value') == b'durable\n'
                        execute(home, 'web', '/bin/netprobe', 'resolve', name+'.trames')
                        op(home, 'env', 'stop', name)
                        op(home, 'env', 'remove', name, '--yes')
                        execute(home, 'web', '/bin/netprobe', 'resolve', name+'.trames', ok=False)
                    op(home, 'env', 'disable', 'database', '--now')
                    assert not Path(initial['service']['path']).exists()
                    assert not inspect(home, 'database')['service']['installed']
                    op(home, 'env', 'start', 'database')
                    resolve('database')
                    op(home, 'env', 'stop', 'database')
                    if a.systemd_rootfs:
                        op(home, 'env', 'create', 'init', '--rootfs', a.systemd_rootfs, '--init', 'systemd', '--cpus', cpus, '--memory', 'auto')
                        select_transport('init')
                        op(home, 'env', 'enable', 'init', '--now')
                        script = """for n in $(seq 1 100); do test -S /run/systemd/private && break; sleep .1; done
cat > /run/systemd/system/trames-user-stop.service <<'UNIT'
[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/sh -c 'echo started > /root/user-service-started'
ExecStop=/bin/sh -c 'echo graceful > /root/user-service-stopped'
UNIT
systemctl daemon-reload
systemctl start trames-user-stop.service
systemctl is-active trames-user-stop.service
"""
                        assert b'active' in execute(home, 'init', '/bin/sh', '-ec', script)
                        op(home, 'env', 'stop', 'init')
                        state = inspect(home, 'init')
                        assert state['last_exit']['code'] == 0 and state['last_exit']['error'] is None, state
                        assert op(home, 'read-file', '--store', Path(state['directory'])/'store', '--', '/root/user-service-stopped') == b'graceful\n'
                        op(home, 'env', 'disable', 'init')
                    (case / 'results.json').write_text(json.dumps(dict(passed=True, unit=unit, initial_pid=old_pid, recovered_pid=pid, original_systemd=bool(a.systemd_rootfs), login_target_dependency=True, fresh_login_tested=False), indent=2)+'\n')
                    print(label+': passed', flush=True)
                finally:
                    # Only this registry's UUIDs/units are touched, including failed startup.
                    raw = subprocess.run([runner, 'env', 'ls', '--json'], env=env | {'TRAMES_HOME': str(home)}, capture_output=True)
                    for state in json.loads(raw.stdout or b'[]'):
                        if state.get('service'):
                            journal = subprocess.run(['journalctl', '--user', '--unit', state['service']['unit'], '--no-pager', '--lines=250'],env=env,capture_output=True)
                            (case/(state['uuid']+'.journal')).write_bytes(journal.stdout+journal.stderr)
                        subprocess.run([runner, 'env', 'disable', state['uuid'], '--now'], env=env | {'TRAMES_HOME': str(home)}, capture_output=True, timeout=100)
                        subprocess.run([runner, 'env', 'stop', state['uuid']], env=env | {'TRAMES_HOME': str(home)}, capture_output=True, timeout=100)
                    logs = case / 'logs'
                    logs.mkdir()
                    if (home/'logs').exists():
                        for log in (home/'logs').iterdir():
                            shutil.copy2(log, logs/log.name)
                    for path in dropins:
                        path.unlink(missing_ok=True)
                        path.parent.rmdir()
                    subprocess.run(['systemctl', '--user', 'daemon-reload'], env=env, capture_output=True)
                    for p in children:
                        if p.poll() is None:
                            p.terminate()
                            try:
                                p.wait(timeout=5)
                            except subprocess.TimeoutExpired:
                                p.kill()
                                p.wait()
    (out/'results.json').write_text(json.dumps(dict(passed=True, complete=True, runner_sha256=d.digest(runner), operations=len(rows), original_systemd=bool(a.systemd_rootfs)), indent=2)+'\n')


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--runner', type=Path, required=True)
    p.add_argument('--receipt', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--systemd-rootfs', type=Path)
    p.add_argument('--case', action='append')
    main(p.parse_args())
