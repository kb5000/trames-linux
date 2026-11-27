#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Offline systemd/Docker acceptance through a real named-network host listener."""
import argparse
import hashlib
import http.client
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main(a):
    out = a.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    runner = out / 'runner'
    shutil.copy2(a.runner, runner)
    runner.chmod(0o555)
    receipt = json.loads(a.receipt.read_text())
    assert digest(runner) == receipt['fingerprint']['binary_sha256']
    shutil.copy2(a.receipt, out / 'build-receipt.json')
    shutil.copy2(__file__, out / 'harness.py')
    inputs = ['acceptance/container.tar', 'usr/bin/docker', 'usr/sbin/dockerd', 'usr/bin/containerd', 'usr/lib/systemd/systemd']
    (out / 'rootfs-inputs.json').write_text(json.dumps({p: digest(a.rootfs / p) for p in inputs}, indent=2) + '\n')
    rows = []
    for transport in (a.transport or ['smccc', 'mmio']):
        case = out / transport
        case.mkdir()
        with tempfile.TemporaryDirectory(prefix='registry-', dir=case) as temporary:
            home = Path(temporary)
            env = {k: v for k, v in os.environ.items() if k not in ['DISPLAY', 'WAYLAND_DISPLAY', 'TRAMES_HOME']}
            env.update(TRAMES_HOME=str(home), TRAMES_KVM_TRANSPORT=transport)

            def run(*args):
                began = time.monotonic()
                r = subprocess.run([str(runner), *map(str, args)], env=env, capture_output=True, text=True, timeout=180)
                rows.append(dict(transport=transport, argv=list(map(str, args)), code=r.returncode, stdout=r.stdout, stderr=r.stderr, seconds=time.monotonic()-began))
                (out / 'operations.json').write_text(json.dumps(rows, indent=2) + '\n')
                assert r.returncode == 0, rows[-1]
                return r.stdout

            def execute(script):
                return run('-e', 'services', 'exec', '--', '/bin/sh', '-ec', script)

            with socket.socket() as s:
                s.bind(('127.0.0.1', 0))
                port = s.getsockname()[1]

            def host_http():
                c = http.client.HTTPConnection('127.0.0.1', port, timeout=10)
                try:
                    c.request('GET', '/')
                    r = c.getresponse()
                    assert r.status == 200 and r.read() == b'named-network-docker\n'
                finally:
                    c.close()

            try:
                run('config', '--global', 'set', 'defaults.components.vulkan', 'false', 'defaults.display.windows', 'false')
                if a.topology == 'bridge':
                    run('network', 'create', 'services')
                run('env', 'create', 'services', '--rootfs', a.rootfs, '--init', a.init, '--cpus', str(a.cpus), '--memory', 'auto')
                if a.topology == 'bridge':
                    run('-e', 'services', 'config', 'set', 'network.mode', 'bridge', 'network.bridge', 'services')
                    run('env', 'publish', 'services', port, '18080')
                run('env', 'start', 'services')
                bootstrap = """for n in $(seq 1 100); do test -S /run/systemd/private && break; sleep .1; done
systemctl start docker
for n in $(seq 1 100); do docker info >/dev/null 2>&1 && break; sleep .1; done
test "$(docker info --format '{{.DefaultRuntime}} {{.Driver}}')" = 'runc overlay2'
""" if a.init == 'systemd' else """mount -t tmpfs -o mode=755 tmpfs /run
mkdir -p /dev/shm
mount -t tmpfs -o size=64m,mode=1777 tmpfs /dev/shm
/usr/sbin/dockerd >/root/dockerd.log 2>&1 &
for n in $(seq 1 100); do docker info >/dev/null 2>&1 && break; sleep .1; done
test "$(docker info --format '{{.DefaultRuntime}} {{.Driver}}')" = 'runc overlay2'
"""
                execute(bootstrap + """docker import /acceptance/container.tar trames-acceptance:local
docker network create acceptance-net
docker volume create acceptance-data
docker run -d --name web --network acceptance-net -v acceptance-data:/data -p 18080:8080 trames-acceptance:local /bin/sh -ec 'test "$$" = 1; grep -Eq "^Seccomp:[[:space:]]+2$" /proc/self/status; printf persistent >/data/value; printf "named-network-docker\\n" >/www/index.html; exec /bin/httpd -f -p 8080 -h /www'
for n in $(seq 1 100); do curl -fsS http://127.0.0.1:18080/ && exit 0; sleep .1; done
exit 1""".replace("18080", str(port) if a.topology == "host" else "18080"))
                host_http()
                assert execute('docker exec web /bin/cat /data/value').strip() == 'persistent'
                assert execute('docker run --rm --network acceptance-net trames-acceptance:local /bin/wget -qO- http://web:8080/') == 'named-network-docker\n'
                execute('docker stop -t 2 web')
                c = http.client.HTTPConnection('127.0.0.1', port, timeout=5)
                try:
                    c.request('GET', '/')
                    response = c.getresponse()
                    assert response.status != 200, 'stopped container still served HTTP'
                except (OSError, http.client.HTTPException):
                    pass
                finally:
                    c.close()
                restart = 'systemctl restart docker' if a.init == 'systemd' else "pkill -TERM -x dockerd; for n in $(seq 1 100); do pgrep -x dockerd >/dev/null || break; sleep .1; done; /usr/sbin/dockerd >/root/dockerd.log 2>&1 &"
                execute(restart + "\nfor n in $(seq 1 100); do docker info >/dev/null 2>&1 && break; sleep .1; done; test \"$(docker inspect --format '{{.State.Running}}' web)\" = false")
                execute('docker start web; for n in $(seq 1 100); do curl -fsS http://127.0.0.1:18080/ && exit 0; sleep .1; done; exit 1'.replace('18080', str(port) if a.topology == 'host' else '18080'))
                host_http()
                assert execute('docker exec web /bin/cat /data/value').strip() == 'persistent'
                execute('docker stop -t 2 web; docker rm web; docker volume rm acceptance-data; docker network rm acceptance-net; docker image rm trames-acceptance:local')
                run('env', 'stop', 'services')
                with socket.socket() as s:
                    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                    s.bind(('127.0.0.1', port))
                if a.topology == 'bridge':
                    run('network', 'remove', 'services')
                print(transport + ': ' + a.init + ' -> Docker -> host HTTP passed', flush=True)
            except Exception:
                try:
                    execute('cat /root/dockerd.log 2>/dev/null || true; journalctl --no-pager -u docker -u containerd -n 100 || true; mount; ls -ld /dev/shm; cat /proc/sys/kernel/threads-max; docker inspect web || true')
                except Exception:
                    pass
                raise
            finally:
                subprocess.run([str(runner), 'env', 'stop', 'services'], env=env, capture_output=True, timeout=65)
                if (home / 'logs').exists():
                    shutil.copytree(home / 'logs', case / 'logs')
    (out / 'results.json').write_text(json.dumps(dict(passed=True, cpus=a.cpus, init=a.init, topology=a.topology, runner_sha256=digest(runner), operations=len(rows)), indent=2) + '\n')


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--runner', type=Path, required=True)
    p.add_argument('--receipt', type=Path, required=True)
    p.add_argument('--rootfs', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--cpus', type=int, choices=[1, 4], default=4)
    p.add_argument('--topology', choices=['bridge', 'host'], default='bridge')
    p.add_argument('--init', choices=['shell', 'systemd'], default='systemd')
    p.add_argument('--transport', action='append', choices=['smccc', 'mmio'])
    main(p.parse_args())
