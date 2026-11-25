#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Real software X11/Wayland WSI and original Vulkan ELF/KVM pixel comparison.

Preparation needs native Vulkan/XCB/Wayland headers, wayland-scanner, Xvfb and
Weston. The supplied original guest ICD must target this host's guest libc.
No physical GPU, bare-metal timing or desktop-compositor claim is made here.
"""
import argparse
import contextlib
import json
import os
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'runtime/tests/acceptance'))
from common import digest, save, tree_digest
from run import receipt, execute


def compile_fixture(output, name, args):
    with (output/(name+'.build.log')).open('wb') as log:
        subprocess.run([str(v) for v in args], check=True, stdout=log, stderr=subprocess.STDOUT)


def dependencies(source, root, copied):
    source=Path(source)
    for line in subprocess.check_output(['ldd',source],text=True).splitlines():
        if 'not found' in line:raise RuntimeError('missing ELF dependency: '+line)
        for word in line.split():
            if not word.startswith('/') or word in copied:continue
            copied.add(word);target=root/word.lstrip('/');target.parent.mkdir(parents=True,exist_ok=True)
            shutil.copy2(word,target);dependencies(word,root,copied)


@contextlib.contextmanager
def compositor(argv, output, env, pass_fds=()):
    with output.open('wb') as log:
        process=subprocess.Popen(argv,env=env,stdout=log,stderr=subprocess.STDOUT,pass_fds=pass_fds,start_new_session=True)
        try:yield process
        finally:
            if process.poll() is None:
                os.killpg(process.pid,signal.SIGTERM)
                try:process.wait(timeout=5)
                except subprocess.TimeoutExpired:os.killpg(process.pid,signal.SIGKILL);process.wait()


def ready_output(process, expected, log, seconds=40):
    deadline=time.monotonic()+seconds
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdout,selectors.EVENT_READ)
        data=b''
        while expected not in data:
            remaining=deadline-time.monotonic()
            if remaining<=0 or not selector.select(remaining):raise TimeoutError('missing frame-ready output')
            block=os.read(process.stdout.fileno(),65536)
            if not block:raise RuntimeError('renderer exited before frame-ready')
            data+=block;log.write(block);log.flush()


def render(case, argv, env, probe, title, user_close=False):
    case.mkdir();code=None
    with (case/'stdout').open('wb') as stdout,(case/'stderr').open('wb') as stderr:
        process=subprocess.Popen([str(v) for v in argv],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=stderr,start_new_session=True)
        try:
            ready_output(process,b'wsi-ready frames=16\n',stdout)
            deadline=time.monotonic()+5
            while True:
                pixels=subprocess.run([probe,title],env=env,capture_output=True,timeout=5)
                (case/'pixels.stdout').write_bytes(pixels.stdout);(case/'pixels.stderr').write_bytes(pixels.stderr)
                if pixels.returncode==0:break
                if time.monotonic()>=deadline:raise RuntimeError('native server pixels differ')
                time.sleep(0.02)
            if pixels.stdout!=b'pixels: 320x240 green\n':raise RuntimeError('unexpected native pixel geometry')
            if user_close:
                subprocess.run([probe,title,'close'],env=env,check=True,timeout=5)
                # Keep the guest's original input pipe open until the host close
                # event cancels the session. communicate() would send EOF first.
                process.wait(timeout=15)
            else:process.stdin.write(b'q');process.stdin.flush()
            rest,_=process.communicate(timeout=15);stdout.write(rest);stdout.flush();code=process.returncode
            if code:raise RuntimeError('renderer exit '+str(code))
            if not user_close and b'wsi: ok\n' not in (case/'stdout').read_bytes():raise RuntimeError('renderer did not finish teardown')
            return dict(status='passed',exit_code=code,frames=16,pixels_sha256=digest(case/'pixels.stdout'),user_close=user_close)
        except Exception as error:
            if process.poll() is None:os.killpg(process.pid,signal.SIGKILL)
            if not process.stdout.closed:
                if process.stdin.closed:process.stdin=None
                rest,_=process.communicate();stdout.write(rest)
            return dict(status='failed',exit_code=process.returncode,reason=str(error))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runner',type=Path)
    parser.add_argument('--receipt',type=Path)
    parser.add_argument('--guest-icd',type=Path)
    parser.add_argument('--native-only',action='store_true',help='Run host targets/rendering only; makes no guest or KVM claim')
    parser.add_argument('--unit-binary',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    a=parser.parse_args()
    if not a.native_only and not all([a.runner,a.receipt,a.guest_icd]):parser.error('KVM acceptance requires --runner, --receipt and --guest-icd')
    a.output=a.output.resolve();a.output.mkdir(parents=True,exist_ok=False)
    runner=a.runner.resolve() if a.runner else None;icd=a.guest_icd.resolve() if a.guest_icd else None;unit=a.unit_binary.resolve();build=receipt(a.receipt.resolve(),runner) if not a.native_only else None
    out=a.output;cc=os.environ.get('CC','cc');fixtures=ROOT/'tests/fixtures'
    compile_fixture(out,'native',[cc,'-O2','-Wall','-Wextra','-Werror',fixtures/'vulkan-wsi.c','-lvulkan','-lxcb','-o',out/'native'])
    if not a.native_only:compile_fixture(out,'guest',[cc,'-O2','-Wall','-Wextra','-Werror','-DTRAMES_WSI_GUEST',fixtures/'vulkan-wsi.c','-lvulkan','-o',out/'guest'])
    compile_fixture(out,'probe',[cc,'-O2','-Wall','-Wextra','-Werror',fixtures/'x11-probe.c','-lxcb','-o',out/'probe'])
    xml=Path(subprocess.check_output(['pkg-config','--variable=pkgdatadir','wayland-protocols'],text=True).strip())/'stable/xdg-shell/xdg-shell.xml'
    subprocess.run(['wayland-scanner','client-header',xml,out/'xdg-shell-client-protocol.h'],check=True)
    subprocess.run(['wayland-scanner','private-code',xml,out/'xdg-shell-protocol.c'],check=True)
    compile_fixture(out,'wayland',[cc,'-O2','-Wall','-Wextra','-Werror','-shared','-fPIC','-I',out,fixtures/'wayland-target.c',out/'xdg-shell-protocol.c','-lwayland-client','-o',out/'wayland-target.so'])
    root=out/'rootfs';(root/'test').mkdir(parents=True)
    if not a.native_only:
        shutil.copy2(out/'guest',root/'test/wsi');copied=set()
        dependencies(out/'guest',root,copied);dependencies(icd,root,copied)
    for path in ['usr/lib','usr/share/vulkan/icd.d','tmp']:(root/path).mkdir(parents=True,exist_ok=True)
    (root/'tmp').chmod(0o1777)
    save(out/'inputs.json',dict(build=build,guest_icd_sha256=digest(icd) if icd else None,unit_binary_sha256=digest(unit),rootfs_sha256=tree_digest(root),controller_sha256=digest(Path(__file__)),fixture_sources={p.name:digest(p) for p in [fixtures/'vulkan-wsi.c',fixtures/'x11-probe.c',fixtures/'wayland-target.c']},performance_qualified=False,physical_gpu_qualified=False))
    if not a.native_only:
        doctor=subprocess.run([runner,'doctor','--json','--require-kvm'],capture_output=True,timeout=10)
        (out/'doctor.stdout').write_bytes(doctor.stdout);(out/'doctor.stderr').write_bytes(doctor.stderr)
        if doctor.returncode:raise RuntimeError('real KVM preflight failed')
    runtime=out/'xdg-runtime';runtime.mkdir(mode=0o700)
    env=os.environ|{'XDG_RUNTIME_DIR':str(runtime),'WAYLAND_DISPLAY':'trames-wsi-validation','TRAMES_WAYLAND_FIXTURE':str(out/'wayland-target.so')}
    rows=[];read_fd,write_fd=os.pipe()
    with compositor(['Xvfb','-displayfd',str(write_fd),'-screen','0','1024x768x24','-nolisten','tcp','-noreset'],out/'xvfb.log',env,(write_fd,)):
        os.close(write_fd)
        with os.fdopen(read_fd,'rb',buffering=0) as stream,selectors.DefaultSelector() as selector:
            selector.register(stream,selectors.EVENT_READ)
            if not selector.select(10):raise TimeoutError('Xvfb startup')
            env['DISPLAY']=':'+stream.readline().decode().strip()
        with compositor(['weston','--backend=headless','--renderer=pixman','--socket='+env['WAYLAND_DISPLAY'],'--idle-time=0'],out/'weston.log',env) as weston:
            deadline=time.monotonic()+10
            while not (runtime/env['WAYLAND_DISPLAY']).exists():
                if weston.poll() is not None or time.monotonic()>deadline:raise RuntimeError('Weston startup failed')
                time.sleep(0.02)
            with (out/'native-targets.log').open('wb') as log:
                tests=subprocess.run([unit,'display::','--ignored','--test-threads=1'],env=env,stdout=log,stderr=subprocess.STDOUT,timeout=30)
            test_log=(out/'native-targets.log').read_bytes()
            targets=dict(id='native-target-lifetimes',status='passed' if tests.returncode==0 and b'2 passed;' in test_log else 'failed')
            rows.append(targets)
            native=render(out/'native-case',[out/'native'],env,out/'probe','Trames WSI native');rows.append(dict(id='native-render',**native))
            headless_env=env|{'DISPLAY':'invalid-no-display','WAYLAND_DISPLAY':'invalid-no-display'}
            native_headless=out/'native-headless';native_headless.mkdir()
            headless=execute([out/'native','headless'],headless_env,native_headless,30)
            headless_stdout=(native_headless/'stdout').read_bytes()
            headless_passed=headless['status']=='completed' and headless['exit_code']==0 and headless_stdout==b'headless: ok\n'
            rows.append(dict(id='native-headless',**headless)|dict(status='passed' if headless_passed else headless['status'] if headless['status']!='completed' else 'failed'))
            for transport in ([] if a.native_only else ['smccc','mmio']):
                for cpus in [1,4]:
                    headless_case=out/f'headless-{transport}-{cpus}';headless_case.mkdir()
                    if headless_passed:
                        scratch=out/(headless_case.name+'-rootfs');shutil.copytree(root,scratch)
                        try:
                            result=execute([runner,'run','--cpus',str(cpus),'--rootfs',scratch,'--vulkan-icd',icd,'--timeout-ms','30000','--setenv','VK_DRIVER_FILES=/usr/share/vulkan/icd.d/trames.json','--','/test/wsi','headless'],headless_env|{'TRAMES_KVM_TRANSPORT':transport},headless_case,35)
                            passed=result['status']=='completed' and result['exit_code']==0 and (headless_case/'stdout').read_bytes()==headless_stdout
                            rows.append(dict(id=headless_case.name,**result)|dict(status='passed' if passed else result['status'] if result['status']!='completed' else 'failed'))
                        finally:shutil.rmtree(scratch)
                    else:rows.append(dict(id=headless_case.name,status='reference_failed'))
                    for close in ([False,True] if cpus==4 else [False]):
                        name=f'kvm-{transport}-{cpus}'+('-close' if close else '')
                        if native['status']!='passed':rows.append(dict(id=name,status='reference_failed'));continue
                        # Each session gets its own imported file tree; no frozen
                        # input, prior output, host window or ICD changes mid-run.
                        scratch=out/(name+'-rootfs');shutil.copytree(root,scratch)
                        try:
                            command=[runner,'run','--cpus',str(cpus),'--rootfs',scratch,'--vulkan-icd',icd,'--display','x11','--display-size','320x240','--timeout-ms','50000','--setenv','VK_DRIVER_FILES=/usr/share/vulkan/icd.d/trames.json','--','/test/wsi']
                            row=render(out/name,command,env|{'TRAMES_KVM_TRANSPORT':transport},out/'probe','Trames Linux',close)
                            rows.append(dict(id=name,**row));print(rows[-1],flush=True)
                        finally:shutil.rmtree(scratch)
    save(out/'results.json',dict(cases=rows,passed=sum(r['status']=='passed' for r in rows),planned=len(rows),performance_qualified=False,physical_gpu_qualified=False))
    return 0 if all(r['status']=='passed' for r in rows) else 1


if __name__=='__main__':raise SystemExit(main())
