#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Actual X11 pixels/input through the shared SHM compositor; optional ARM64 KVM.

Native-only exercises the original libwayland client and production presenter,
without guest/KVM or physical GPU claims. KVM additionally runs original distro
Xwayland, two terminal applications, and the same protocol/input fixtures.
"""
import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import tempfile
import time

ROOT=Path(__file__).resolve().parents[1]
TITLE='Trames desktop acceptance'
def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def run(argv,**kw):return subprocess.run([str(x) for x in argv],check=True,**kw)
def dependencies(source,root,copied):
    source=Path(source)
    for line in subprocess.check_output(['ldd',source],text=True).splitlines():
        if 'not found' in line:raise RuntimeError('missing dependency: '+line)
        for word in line.split():
            if word.startswith('/') and word not in copied:
                copied.add(word);target=root/word.lstrip('/');target.parent.mkdir(parents=True,exist_ok=True)
                shutil.copy2(word,target);dependencies(word,root,copied)
def install(source,target,root,copied):
    destination=root/target.lstrip('/');destination.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(source,destination);dependencies(source,root,copied)

class Process:
    def __init__(self,argv,env,output):
        self.path=output;self.path.mkdir();(output/'command.json').write_text(json.dumps([str(x) for x in argv],indent=2)+'\n');self.err=(output/'stderr').open('wb');self.log=(output/'stdout').open('wb')
        self.child=subprocess.Popen([str(x) for x in argv],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.err,start_new_session=True)
        os.set_blocking(self.child.stdout.fileno(),False);self.data=b''
    def drain(self,seconds=0):
        with selectors.DefaultSelector() as s:
            s.register(self.child.stdout,selectors.EVENT_READ)
            if s.select(seconds):
                while True:
                    try:block=os.read(self.child.stdout.fileno(),65536)
                    except BlockingIOError:break
                    if not block:break
                    self.log.write(block);self.log.flush();self.data+=block
    def expect(self,marker,seconds=30,after=0):
        deadline=time.monotonic()+seconds
        while marker not in self.data[after:]:
            self.drain(min(.1,max(0,deadline-time.monotonic())))
            if marker in self.data[after:]:break
            if time.monotonic()>deadline or self.child.poll() is not None:
                self.drain();raise RuntimeError('missing '+repr(marker)+'; '+repr(self.data[-300:]))
    def finish(self):
        self.child.wait(timeout=20);self.drain();assert self.child.returncode==0,(self.child.returncode,self.path)
    def close(self):
        if self.child.poll() is None:os.killpg(self.child.pid,signal.SIGKILL);self.child.wait()
        self.drain();self.child.stdout.close();self.child.stdin.close();self.log.close();self.err.close()

def probe(binary,env,title,*args):return subprocess.check_output([str(binary),title,*map(str,args)],env=env,stderr=subprocess.PIPE)
def pixels(binary,env,title,expected):
    deadline=time.monotonic()+15
    while True:
        try:
            value=probe(binary,env,title,'pixels')
            if value==expected:return value.decode().strip()
        except subprocess.CalledProcessError:pass
        if time.monotonic()>deadline:raise RuntimeError('pixel mismatch: '+repr(locals().get('value'))+' expected '+repr(expected))
        time.sleep(.02)
def ticks(pid):
    fields=Path(f'/proc/{pid}/stat').read_text().rsplit(')',1)[1].split();return int(fields[11])+int(fields[12])

def interaction(process,probe_binary,env,x11=False):
    checks=[]
    checks.append(pixels(probe_binary,env,TITLE,b'pixels 240 160 20c040\n'))
    if not x11:process.expect(b'frame ')
    probe(probe_binary,env,TITLE,'focus');process.expect(b'focus-in')
    probe(probe_binary,env,TITLE,'motion',80,60)
    probe(probe_binary,env,TITLE,'key',38,1);probe(probe_binary,env,TITLE,'key',38,0)
    process.expect(b'key 30 1\n');process.expect(b'key 30 0\n')
    checks.append(pixels(probe_binary,env,TITLE,b'pixels 240 160 c04020\n'))
    probe(probe_binary,env,TITLE,'button',1,1);probe(probe_binary,env,TITLE,'button',1,0)
    process.expect(b'button '+(b'1' if x11 else b'272')+b' 1\n');process.expect(b'button '+(b'1' if x11 else b'272')+b' 0\n')
    checks.append(pixels(probe_binary,env,TITLE,b'pixels 240 160 2040c0\n'))
    probe(probe_binary,env,TITLE,'button',4,1);probe(probe_binary,env,TITLE,'button',4,0)
    process.expect(b'button 4 1\n' if x11 else b'scroll 0 -10\n')
    start=len(process.data);probe(probe_binary,env,TITLE,'key',50,1)
    if not x11:process.expect(b'modifiers 1 0\n',after=start)
    probe(probe_binary,env,TITLE,'unfocus');process.expect(b'focus-out',after=start)
    probe(probe_binary,env,TITLE,'key',50,0);probe(probe_binary,env,TITLE,'focus')
    if not x11:process.expect(b'focus-in keys=0\n',after=start);process.expect(b'modifiers 0 0\n',after=start)
    probe(probe_binary,env,TITLE,'resize',320,200);process.expect(b'size 320 200\n')
    checks.append(pixels(probe_binary,env,TITLE,b'pixels 320 200 2040c0\n'))
    if not x11:process.expect(b'frame 3\n')
    probe(probe_binary,env,TITLE,'expose');checks.append(pixels(probe_binary,env,TITLE,b'pixels 320 200 2040c0\n'))
    process.drain(.15);before=ticks(process.child.pid);time.sleep(1);idle=(ticks(process.child.pid)-before)/os.sysconf('SC_CLK_TCK')
    if idle>.25:raise RuntimeError(f'idle consumed {idle}s CPU in 1s')
    probe(probe_binary,env,TITLE,'close');process.expect(b'desktop-done');process.finish()
    return dict(pixels=checks,idle_cpu_seconds=idle,input=True,focus_loss=True,resize=True,expose=True,close=True)

KEYS={**dict(zip('1234567890',range(10,20))),**dict(zip('qwertyuiop',range(24,34))),**dict(zip('asdfghjkl',range(38,47))),**dict(zip('zxcvbnm',range(52,59))),' ':65,'/':61,'.':60,'-':20,'=':21,'\n':36}
def type_text(binary,env,title,text):
    for char in text:
        shift=char in '>'
        code=60 if char=='>' else KEYS[char]
        if shift:probe(binary,env,title,'key',50,1)
        probe(binary,env,title,'key',code,1);probe(binary,env,title,'key',code,0)
        if shift:probe(binary,env,title,'key',50,0)

@contextlib.contextmanager
def xvfb(output):
    read,write=os.pipe()
    with (output/'xvfb.log').open('wb') as log:
        child=subprocess.Popen(['Xvfb','-displayfd',str(write),'-screen','0','1024x768x24','-nolisten','tcp','-noreset'],pass_fds=(write,),stdout=log,stderr=subprocess.STDOUT)
        os.close(write)
        try:
            with os.fdopen(read,'rb',buffering=0) as stream,selectors.DefaultSelector() as s:
                s.register(stream,selectors.EVENT_READ)
                if not s.select(10):raise TimeoutError('Xvfb startup')
                yield ':'+stream.readline().decode().strip()
        finally:child.terminate();child.wait(timeout=10)

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--unit-binary',type=Path,required=True);p.add_argument('--runner',type=Path);p.add_argument('--native-only',action='store_true');p.add_argument('--output',type=Path,required=True);p.add_argument('--case',action='append',choices=['native-wayland']+[f'kvm-{t}-{c}-{w}' for t in ['smccc','mmio'] for c in [1,4] for w in ['wayland','xwayland']]+['terminal-weston','terminal-xterm','store-create','store-reopen']);a=p.parse_args()
    if not a.native_only and not a.runner:p.error('KVM requires --runner')
    a.output=a.output.resolve();a.output.mkdir(parents=True,exist_ok=False);out=a.output;cc=os.environ.get('CC','cc');fixtures=ROOT/'tests/fixtures'
    # Freeze executable bytes before any child starts; concurrent Cargo links
    # cannot change a matrix or the identity recorded after it.
    for attribute,name in [('unit_binary','unit'),('runner','runner')]:
        source=getattr(a,attribute)
        if source:
            target=out/name;shutil.copy2(source.resolve(),target);target.chmod(0o555)
            assert digest(source)==digest(target),'executable changed while freezing'
            setattr(a,attribute,target)

    xml=Path(subprocess.check_output(['pkg-config','--variable=pkgdatadir','wayland-protocols'],text=True).strip())/'stable/xdg-shell/xdg-shell.xml'
    run(['wayland-scanner','client-header',xml,out/'xdg-shell-client-protocol.h']);run(['wayland-scanner','private-code',xml,out/'xdg-shell-protocol.c'])
    build=[cc,'-O2','-Wall','-Wextra','-Werror']
    run(build+['-I',out,fixtures/'desktop-client.c',out/'xdg-shell-protocol.c','-lwayland-client','-o',out/'client'])
    run(build+[fixtures/'desktop-probe.c','-lxcb','-lxcb-xtest','-o',out/'probe'])
    rows=[];rootfs_files={}
    def wanted(name):return not a.case or name in a.case
    def record(row):
        rows.append(row);(out/'results.json').write_text(json.dumps(dict(passed=False,rows=rows,complete=False),indent=2)+'\n')
    if a.native_only and a.case and a.case!=['native-wayland']:p.error('native-only accepts only native-wayland')
    with xvfb(out) as display:
        env=os.environ|{'DISPLAY':display,'TRAMES_DESKTOP_CLIENT':str(out/'client')}
        if wanted('native-wayland'):
            child=Process([a.unit_binary.resolve(),'desktop::tests::native_wayland_desktop','--ignored','--exact','--nocapture'],env,out/'native')
            try:record(dict(id='native-wayland',status='passed',**interaction(child,out/'probe',env)))
            finally:child.close()
        if not a.native_only:
            runner=a.runner.resolve();doctor=subprocess.check_output([runner,'doctor','--json','--require-kvm']);(out/'doctor.json').write_bytes(doctor)
            run(build+[fixtures/'desktop-x11.c','-lxcb','-o',out/'x11-client'])
            run([os.environ.get('MUSL_CC','musl-gcc'),'-static','-O2','-Wall','-Wextra','-Werror',ROOT/'tools/xwayland-launcher.c','-o',out/'launcher'])
            root=out/'rootfs';root.mkdir();copied=set()
            for source,target in [(out/'client','/test/client'),(out/'x11-client','/test/x11-client'),('/bin/sh','/bin/sh'),('/bin/sleep','/bin/sleep'),('/bin/cat','/bin/cat'),('/bin/rm','/bin/rm'),('/usr/bin/Xwayland','/usr/bin/Xwayland'),('/usr/bin/xkbcomp','/usr/bin/xkbcomp'),('/usr/bin/weston-terminal','/usr/bin/weston-terminal'),('/usr/bin/xterm','/usr/bin/xterm')]:install(source,target,root,copied)
            for directory in ['tmp','usr/bin','run','root','dev','proc','usr/share/fonts','etc/fonts']:(root/directory).mkdir(parents=True,exist_ok=True)
            (root/'tmp').chmod(0o1777)
            shutil.copytree('/usr/share/X11/xkb',root/'usr/share/X11/xkb',dirs_exist_ok=True)
            shutil.copytree('/etc/fonts',root/'etc/fonts',dirs_exist_ok=True)
            shutil.copytree('/usr/share/weston',root/'usr/share/weston',dirs_exist_ok=True)
            shutil.copytree('/usr/share/icons/Adwaita/cursors',root/'usr/share/icons/Adwaita/cursors',dirs_exist_ok=True)
            font=Path(subprocess.check_output(['fc-match','-f','%{file}','monospace'],text=True));target=root/str(font).lstrip('/');target.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(font,target)
            rootfs_files={str(f.relative_to(root)):digest(f) for f in sorted(root.rglob('*')) if f.is_file()}
            for transport in ['smccc','mmio']:
                for cpus in [1,4]:
                    for x11 in [False,True]:
                        name=f'kvm-{transport}-{cpus}-'+('xwayland' if x11 else 'wayland')
                        if not wanted(name):continue
                        command='trames-xwayland :0 -ac >&2 & xpid=$!; trap "kill $xpid" EXIT; while [ ! -S /tmp/.X11-unix/X0 ]; do sleep .05; done; DISPLAY=:0 /test/x11-client' if x11 else 'exec /test/client'
                        argv=[runner,'run','--rootfs',root,'--desktop','x11','--xwayland-launcher',out/'launcher','--cpus',cpus,'--timeout-ms','60000','--','/bin/sh','-ec',command]
                        child=Process(argv,env|{'TRAMES_KVM_TRANSPORT':transport},out/name)
                        try:record(dict(id=name,status='passed',**interaction(child,out/'probe',env,x11)))
                        finally:child.close()
            for reopen in [False,True]:
                name='store-reopen' if reopen else 'store-create'
                if not wanted(name):continue
                store=out/'store'
                if reopen and not store.exists():raise RuntimeError('store-reopen needs store-create in the same invocation')
                command='trames-xwayland :0 -ac >&2 & xpid=$!; trap "kill $xpid" EXIT; while [ ! -S /tmp/.X11-unix/X0 ]; do sleep .05; done; DISPLAY=:0 /test/x11-client' if reopen else 'exec /test/client'
                argv=[runner,'run','--store',store]+([] if reopen else ['--rootfs',root])+['--desktop','x11','--xwayland-launcher',out/'launcher','--cpus','4','--timeout-ms','60000','--','/bin/sh','-ec',command]
                child=Process(argv,env,out/name)
                try:record(dict(id=name,status='passed',**interaction(child,out/'probe',env,reopen)))
                finally:child.close()
            for x11 in [False,True]:
                name='terminal-'+('xterm' if x11 else 'weston');title='Trames terminal acceptance' if x11 else '*'
                if not wanted(name):continue
                prefix='trames-xwayland :0 -ac >&2 & xpid=$!; trap "kill $xpid" EXIT; while [ ! -S /tmp/.X11-unix/X0 ]; do sleep .05; done; DISPLAY=:0 xterm -T "Trames terminal acceptance" -fa monospace -e /bin/sh' if x11 else 'weston-terminal --shell=/bin/sh'
                command='rm -f /tmp/gui-result; '+prefix+' & pid=$!; while [ ! -f /tmp/gui-result ]; do sleep .05; done; cat /tmp/gui-result; kill "$pid"; wait "$pid" || :; echo cli-survived; read reply; [ "$reply" = resume ]'
                # Only this owned scratch rootfs is writable; reset the result each run.
                child=Process([runner,'run','--rootfs',root,'--desktop','x11','--xwayland-launcher',out/'launcher','--cpus','4','--timeout-ms','60000','--','/bin/sh','-ec',command],env,out/name)
                try:
                    deadline=time.monotonic()+30
                    while True:
                        try:probe(out/'probe',env,title,'focus');break
                        except subprocess.CalledProcessError:
                            child.drain(.05)
                            if time.monotonic()>deadline:raise RuntimeError('terminal window missing '+title)
                    type_text(out/'probe',env,title,'echo guiok >/tmp/gui-result\n');child.expect(b'guiok\n');child.expect(b'cli-survived\n');assert child.child.poll() is None
                    child.child.stdin.write(b'resume\n');child.child.stdin.flush();child.finish();record(dict(id=name,status='passed',keyboard_shell_command=True,cli_survives=True))
                finally:child.close()
    inputs=dict(unit_sha256=digest(a.unit_binary),runner_sha256=digest(a.runner) if a.runner else None,controller_sha256=digest(__file__),runtime_pin=json.loads((ROOT/'runtime-version.json').read_text()),sources={str(f.relative_to(ROOT)):digest(f) for f in sorted(fixtures.glob('desktop-*.c'))},launcher_sha256=digest(out/'launcher') if (out/'launcher').exists() else None,launcher_source_sha256=digest(ROOT/'tools/xwayland-launcher.c'),rootfs_files=rootfs_files,physical_gpu_qualified=False,native_only=a.native_only)
    assert rows,'zero desktop cases executed'
    if a.case:assert {r['id'] for r in rows}==set(a.case),'requested cases missing'
    result=dict(passed=True,complete=True,rows=rows,inputs=inputs);(out/'results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(passed=True,cases=len(rows),ids=[r['id'] for r in rows])))
if __name__=='__main__':main()
