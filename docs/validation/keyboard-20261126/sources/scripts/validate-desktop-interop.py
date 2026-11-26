#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Independent host clipboard/IME/native Wayland/Vulkan desktop acceptance.

Original libwayland, X11 and Vulkan clients talk through real sockets. Native
references are separate from ARM64 KVM. Mesa software results never qualify a
physical GPU or performance. All binaries are frozen before executing cases.
"""
import argparse
import contextlib
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
_spec = importlib.util.spec_from_file_location('desktop_acceptance', ROOT/'scripts/validate-desktop.py')
desktop = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(desktop)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def send(process, line):
    process.child.stdin.write((line+'\n').encode())
    process.child.stdin.flush()


@contextlib.contextmanager
def host(case, wayland, tools, shell='kiosk-shell.so', keyboard='', parent_keymap=()):
    with desktop.xvfb(case) as display, tempfile.TemporaryDirectory(prefix='trames-interop-') as runtime:
        env = os.environ | {'DISPLAY':display, 'XDG_RUNTIME_DIR':runtime,
                            'WAYLAND_DISPLAY':'interop', 'XMODIFIERS':'@im=trames-test'}
        if parent_keymap:
            argv=['setxkbmap',*parent_keymap]
            (case/'parent-keymap.command.json').write_text(json.dumps(argv)+'\n')
            subprocess.run(argv,env=env,check=True)
        if not wayland:
            yield env
            return
        config = case/'weston.ini'
        config.write_text('[core]\nidle-time=0\n[input-method]\npath='+str(tools/'input-method')+'\n'+keyboard)
        with (case/'weston.log').open('wb') as log:
            process = subprocess.Popen(['weston','--backend=x11','--renderer=pixman',
                '--shell='+shell,'--width=320','--height=240','--socket=interop','--config='+str(config)],
                env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                deadline = time.monotonic()+10
                while not Path(runtime,'interop').exists():
                    if process.poll() is not None or time.monotonic()>deadline:
                        raise RuntimeError('Weston failed to start')
                    time.sleep(.02)
                while True:
                    try:
                        desktop.probe(tools/'probe',env,'*','focus')
                        break
                    except subprocess.CalledProcessError:
                        if time.monotonic()>deadline:
                            raise
                        time.sleep(.02)
                yield env
            finally:
                if process.poll() is None:
                    os.killpg(process.pid,signal.SIGTERM)
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid,signal.SIGKILL)
                        process.wait()


class Acceptance:
    def __init__(self, args):
        self.args = args
        self.out = args.output.resolve()
        self.out.mkdir(parents=True,exist_ok=False)
        self.tools = self.out/'tools'
        self.tools.mkdir()
        self.rows = []
        self.scratch = set()
        self.binaries = {}
        for key in ['unit_binary','runner','guest_icd']:
            source = getattr(args,key)
            if source:
                target = self.tools/key
                shutil.copy2(source.resolve(),target)
                target.chmod(0o555)
                assert digest(source)==digest(target), 'binary changed while freezing'
                self.binaries[key] = target
        self.root = self.out/'rootfs'

    def compile(self):
        fixtures = ROOT/'tests/fixtures'
        protocols = Path(subprocess.check_output(['pkg-config','--variable=pkgdatadir','wayland-protocols'],text=True).strip())
        xmls = [('xdg-shell', 'stable/xdg-shell/xdg-shell.xml'),
                ('text-input-v3','unstable/text-input/text-input-unstable-v3.xml'),
                ('input-method-v1','unstable/input-method/input-method-unstable-v1.xml')]
        for name, source in xmls:
            subprocess.run(['wayland-scanner','client-header',protocols/source,self.tools/(name+'-client-protocol.h')],check=True)
            subprocess.run(['wayland-scanner','private-code',protocols/source,self.tools/(name+'-protocol.c')],check=True)
        cc = os.environ.get('CC','cc')
        build = [cc,'-O2','-Wall','-Wextra','-Werror','-I'+str(self.tools)]
        xdg = self.tools/'xdg-shell-protocol.c'
        commands = {
            'interop-client':[fixtures/'interop-client.c',xdg,self.tools/'text-input-v3-protocol.c','-lwayland-client'],
            'desktop-client':[fixtures/'desktop-client.c',xdg,'-lwayland-client'],
            'input-method':[fixtures/'interop-input-method.c',self.tools/'input-method-v1-protocol.c','-lwayland-client'],
            'probe':[fixtures/'desktop-probe.c','-lxcb','-lxcb-xtest'],
            'xclip':[fixtures/'interop-xclip.c','-lxcb'],
            'gpu-wayland':['-DTRAMES_WSI_WAYLAND',fixtures/'vulkan-wsi.c',fixtures/'wayland-target.c',xdg,'-lvulkan','-lwayland-client'],
            'gpu-x11':[fixtures/'vulkan-wsi.c','-lvulkan','-lxcb'],
        }
        for name, command in commands.items():
            argv = [str(x) for x in build+command+['-o',self.tools/name]]
            (self.tools/(name+'.command.json')).write_text(json.dumps(argv,indent=2)+'\n')
            with (self.tools/(name+'.build.log')).open('wb') as log:
                subprocess.run(argv,check=True,stdout=log,stderr=subprocess.STDOUT)
        if not self.args.native_only:
            self.root.mkdir()
            copied = set()
            for source, target in [(self.tools/'interop-client','/test/interop'),(self.tools/'gpu-wayland','/test/gpu')]:
                desktop.install(source,target,self.root,copied)
            desktop.dependencies(self.binaries['guest_icd'],self.root,copied)
            for directory in ['tmp','run','root','proc','dev','usr/bin']:
                (self.root/directory).mkdir(parents=True,exist_ok=True)
            (self.root/'tmp').chmod(0o1777)
            doctor = subprocess.run([self.binaries['runner'],'doctor','--json','--require-kvm'],capture_output=True,timeout=10)
            (self.out/'doctor.stdout').write_bytes(doctor.stdout)
            (self.out/'doctor.stderr').write_bytes(doctor.stderr)
            doctor.check_returncode()
        sources = [ROOT/'Cargo.toml',ROOT/'Cargo.lock',ROOT/'runtime-version.json',Path(__file__),ROOT/'scripts/validate-desktop.py']
        for directory in ['crates','runtime/crates','runtime/vendor/wayland-backend','tests/fixtures']:
            sources.extend(p for p in (ROOT/directory).rglob('*') if p.is_file() and 'target' not in p.relative_to(ROOT).parts)
        inputs = dict(binary_sha256={k:digest(v) for k,v in self.binaries.items()},
            native_tools_sha256={name:digest(self.tools/name) for name in commands},
            runtime_pin=json.loads((ROOT/'runtime-version.json').read_text()),
            sources={str(p.relative_to(ROOT)):digest(p) for p in sorted(set(sources))},
            rootfs_files={str(p.relative_to(self.root)):digest(p) for p in self.root.rglob('*') if p.is_file()} if self.root.exists() else {},
            physical_gpu_qualified=False,performance_qualified=False,native_only=self.args.native_only)
        (self.out/'inputs.json').write_text(json.dumps(inputs,indent=2,sort_keys=True)+'\n')

    def guest(self, case, env, wayland, transport, cpus, gpu=False, renderer='auto'):
        if transport is None:
            env = env | {'TRAMES_DESKTOP_HOST':'wayland' if wayland else 'x11',
                'TRAMES_DESKTOP_RENDERER':renderer,'TRAMES_DESKTOP_CLIENT':str(self.tools/'interop-client')}
            argv = [self.binaries['unit_binary'],'desktop::tests::native_wayland_desktop','--ignored','--exact','--nocapture']
        else:
            env = env | {'TRAMES_KVM_TRANSPORT':transport}
            scratch=case/'rootfs'
            shutil.copytree(self.root,scratch)
            self.scratch.add(scratch)
            (case/'rootfs-inputs.json').write_text(json.dumps({str(p.relative_to(scratch)):digest(p) for p in scratch.rglob('*') if p.is_file()},indent=2,sort_keys=True)+'\n')
            argv = [self.binaries['runner'],'run','--rootfs',scratch,'--desktop','wayland' if wayland else 'x11',
                '--desktop-renderer',renderer,'--cpus',str(cpus),'--timeout-ms','60000']
            if gpu:
                argv += ['--vulkan-icd',self.binaries['guest_icd'],'--setenv','VK_DRIVER_FILES=/usr/share/vulkan/icd.d/trames.json']
            argv += ['--','/test/gpu' if gpu else '/test/interop']
        return desktop.Process(argv,env,case/'guest')

    def record(self, name, function):
        if self.args.case and name not in self.args.case:
            return
        case = self.out/name
        case.mkdir()
        try:
            details = function(case) or {}
            self.rows.append(dict(id=name,status='passed',**details))
        except Exception as error:
            self.rows.append(dict(id=name,status='failed',reason=str(error)))
            raise
        finally:
            if case/'rootfs' in self.scratch:
                shutil.rmtree(case/'rootfs')
                self.scratch.remove(case/'rootfs')
            self.save(False)
        print(name+': passed',flush=True)

    def save(self, complete):
        (self.out/'results.json').write_text(json.dumps(dict(complete=complete,rows=self.rows,
            passed=bool(self.rows) and all(r['status']=='passed' for r in self.rows),
            physical_gpu_qualified=False,performance_qualified=False),indent=2)+'\n')

    def x11(self, case, transport=None, cpus=1):
        with host(case,False,self.tools) as env:
            im = desktop.Process([self.binaries['unit_binary'],'interop_tests::host_xim_peer','--ignored','--exact','--nocapture'],env,case/'im')
            guest = owner = None
            try:
                im.expect(b'xim-ready')
                guest = self.guest(case,env,False,transport,cpus)
                guest.expect(b'size ')
                desktop.probe(self.tools/'probe',env,desktop.TITLE,'focus')
                guest.expect(b'focus-in')
                send(guest,'copy')
                guest.expect(b'clipboard-sent')
                value = subprocess.check_output([self.tools/'xclip','get'],env=env,timeout=15)
                assert value=='guest 剪贴板 ✅\n'.encode(), value
                start = len(guest.data)
                owner = desktop.Process([self.tools/'xclip','own','host 剪贴板 🧪'],env,case/'owner')
                owner.expect(b'owner-ready')
                guest.expect(b'selection 1',after=start)
                send(guest,'paste')
                guest.expect('paste host 剪贴板 🧪\n'.encode())
                send(guest,'text')
                im.expect(b'xim-focus')
                for code, marker in [(39,'preedit 拼音'),(36,'commit 中文输入 ✅\n')]:
                    for pressed in [1,0]:
                        desktop.probe(self.tools/'probe',env,desktop.TITLE,'key',code,pressed)
                    guest.expect(marker.encode())
                send(guest,'quit')
                guest.expect(b'interop-done')
                guest.finish()
                return dict(clipboard_both_directions=True,utf8=True,preedit=True,commit=True)
            finally:
                for process in [guest,owner,im]:
                    if process:
                        process.close()

    def wayland(self, case, operation, transport=None, cpus=1, renderer='auto'):
        shell = 'desktop-shell.so' if operation=='ime' else 'kiosk-shell.so'
        with host(case,True,self.tools,shell) as env:
            guest = peer = None
            try:
                if operation=='paste':
                    peer = desktop.Process([self.tools/'interop-client'],env,case/'peer')
                    peer.expect(b'focus-in')
                    for pressed in [1,0]:
                        desktop.probe(self.tools/'probe',env,'*','key',40,pressed)
                    peer.expect(b'key 32 0')
                    send(peer,'value host 剪贴板 🧪')
                    send(peer,'copy')
                    peer.expect(b'copy-ready')
                guest = self.guest(case,env,True,transport,cpus,renderer=renderer)
                guest.expect(b'focus-in')
                if operation=='copy':
                    for pressed in [1,0]:
                        desktop.probe(self.tools/'probe',env,'*','key',40,pressed)
                    guest.expect(b'key 32 0')
                    send(guest,'copy')
                    guest.expect(b'clipboard-sent')
                    peer = desktop.Process([self.tools/'interop-client'],env,case/'peer')
                    peer.expect(b'focus-in')
                    peer.expect(b'selection 1')
                    send(peer,'paste')
                    peer.expect('paste guest 剪贴板 ✅\n'.encode())
                elif operation=='paste':
                    guest.expect(b'selection 1')
                    send(guest,'paste')
                    guest.expect('paste host 剪贴板 🧪\n'.encode())
                elif operation=='ime':
                    send(guest,'text')
                    for marker in ['preedit 拼音','delete 3 0','commit 中文输入 ✅\n']:
                        guest.expect(marker.encode())
                elif operation=='vulkan-shm':
                    guest.expect(b'frame ')
                    desktop.pixels(self.tools/'probe',env,'*',b'pixels 320 240 20c040\n')
                    for pressed in [1,0]:
                        desktop.probe(self.tools/'probe',env,'*','key',38,pressed)
                    guest.expect(b'key 30 1')
                    desktop.pixels(self.tools/'probe',env,'*',b'pixels 320 240 c04020\n')
                send(guest,'quit')
                guest.expect(b'interop-done')
                guest.finish()
                if peer:
                    send(peer,'quit')
                    peer.expect(b'interop-done')
                    peer.finish()
                return dict(operation=operation,native_wayland=True,renderer=renderer)
            finally:
                for process in [guest,peer]:
                    if process:
                        process.close()

    def gpu(self, case, wayland, transport=None, cpus=1):
        with host(case,wayland,self.tools) as env:
            if transport is None:
                argv = [self.tools/('gpu-wayland' if wayland else 'gpu-x11')]
                process = desktop.Process(argv,env,case/'reference')
            else:
                process = self.guest(case,env,wayland,transport,cpus,gpu=True)
            try:
                process.expect(b'wsi-ready frames=16')
                title = '*' if wayland else 'Trames WSI native' if transport is None else 'Trames native Wayland target'
                desktop.pixels(self.tools/'probe',env,title,b'pixels 320 240 00ff00\n')
                process.child.stdin.write(b'q')
                process.child.stdin.flush()
                process.expect(b'wsi: ok')
                process.finish()
                return dict(original_vulkan_frames=16,swapchain_generations=2,pixels=True,teardown=True,
                            gpu_readback=False if transport else None)
            finally:
                process.close()

    def x11_shm_gpu(self, case):
        with host(case,False,self.tools) as env:
            env = env | {'TRAMES_DESKTOP_RENDERER':'vulkan','TRAMES_DESKTOP_CLIENT':str(self.tools/'desktop-client')}
            process = desktop.Process([self.binaries['unit_binary'],'desktop::tests::native_wayland_desktop','--ignored','--exact','--nocapture'],env,case/'guest')
            try:
                return desktop.interaction(process,self.tools/'probe',env)
            finally:
                process.close()

    def run(self):
        self.compile()
        self.record('native-x11-vulkan-reference',lambda c:self.gpu(c,False))
        self.record('native-wayland-vulkan-reference',lambda c:self.gpu(c,True))
        self.record('native-x11-interop',self.x11)
        for operation in ['copy','paste','ime']:
            self.record('native-wayland-'+operation,lambda c,op=operation:self.wayland(c,op))
        self.record('native-x11-vulkan-shm',self.x11_shm_gpu)
        self.record('native-wayland-vulkan-shm',lambda c:self.wayland(c,'vulkan-shm',renderer='vulkan'))
        if not self.args.native_only:
            for transport in ['smccc','mmio']:
                for cpus in [1,4]:
                    prefix=f'kvm-{transport}-{cpus}-'
                    self.record(prefix+'x11-interop',lambda c,t=transport,n=cpus:self.x11(c,t,n))
                    for operation in ['copy','paste','ime']:
                        self.record(prefix+'wayland-'+operation,lambda c,op=operation,t=transport,n=cpus:self.wayland(c,op,t,n))
                    for wayland in [False,True]:
                        self.record(prefix+('wayland' if wayland else 'x11')+'-vulkan',lambda c,w=wayland,t=transport,n=cpus:self.gpu(c,w,t,n))
        assert self.rows, 'no cases executed'
        if self.args.case:
            assert {r['id'] for r in self.rows}==set(self.args.case),'requested cases missing'
        self.save(True)
        print(json.dumps(dict(passed=True,cases=len(self.rows),physical_gpu_qualified=False)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--unit-binary',type=Path,required=True)
    parser.add_argument('--runner',type=Path)
    parser.add_argument('--guest-icd',type=Path)
    parser.add_argument('--native-only',action='store_true')
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--case',action='append')
    args = parser.parse_args()
    if not args.native_only and not (args.runner and args.guest_icd):
        parser.error('KVM acceptance needs --runner and --guest-icd')
    Acceptance(args).run()


if __name__=='__main__':
    main()
