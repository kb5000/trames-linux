#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Original XKB layout/state/repeat acceptance on native references and KVM.

No physical GPU or CPU performance claim. Each guest program interprets actual
protocol keymaps/state using original libxkbcommon or Xlib, without state injection.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import time

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('interop',ROOT/'scripts/validate-desktop-interop.py')
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)


def run(args):
    out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
    tools=out/'tools';tools.mkdir()
    frozen={}
    for name in ['unit_binary','runner','launcher']:
        source=getattr(args,name)
        if source:
            target=tools/name;shutil.copy2(source.resolve(),target);target.chmod(0o555)
            assert m.digest(source)==m.digest(target)
            frozen[name]=target
    protocol=Path(subprocess.check_output(['pkg-config','--variable=pkgdatadir','wayland-protocols'],text=True).strip())/'stable/xdg-shell/xdg-shell.xml'
    subprocess.run(['wayland-scanner','client-header',protocol,tools/'xdg-shell-client-protocol.h'],check=True)
    subprocess.run(['wayland-scanner','private-code',protocol,tools/'xdg-shell-protocol.c'],check=True)
    cc=[os.environ.get('CC','cc'),'-O2','-Wall','-Wextra','-Werror']
    builds={'wayland-client':cc+['-I'+str(tools),ROOT/'tests/fixtures/keyboard-client.c',tools/'xdg-shell-protocol.c','-lwayland-client','-l:libxkbcommon.so.0'],
            'x11-client':cc+shlex.split(os.environ.get('X11_CFLAGS',''))+[ROOT/'tests/fixtures/keyboard-x11.c']+shlex.split(os.environ.get('X11_LIBS','-lX11')),
            'control':cc+shlex.split(os.environ.get('X11_CFLAGS',''))+[ROOT/'tests/fixtures/keyboard-control.c']+shlex.split(os.environ.get('X11_LIBS','-lX11')),
            'probe':cc+[ROOT/'tests/fixtures/desktop-probe.c','-lxcb','-lxcb-xtest']}
    for name,command in builds.items():
        command=[str(v) for v in command+['-o',tools/name]]
        (tools/(name+'.command.json')).write_text(json.dumps(command,indent=2)+'\n')
        with (tools/(name+'.build.log')).open('wb') as log:subprocess.run(command,check=True,stdout=log,stderr=subprocess.STDOUT)
    root=out/'wayland-rootfs';root.mkdir();copied=set()
    m.desktop.install(tools/'wayland-client','/test/keyboard',root,copied)
    for directory in ['tmp','run','root','proc','dev','usr/bin']:(root/directory).mkdir(parents=True,exist_ok=True)
    (root/'tmp').chmod(0o1777)
    shutil.copytree('/usr/share/X11/xkb',root/'usr/share/X11/xkb')
    if args.runner:
        (out/'doctor.json').write_bytes(subprocess.check_output([frozen['runner'],'doctor','--json','--require-kvm']))
        if args.receipt:
            receipt=json.loads(args.receipt.read_text());assert receipt['fingerprint']['binary_sha256']==m.digest(frozen['runner']);shutil.copy2(args.receipt,out/'build-receipt.json')
    sources=[ROOT/'Cargo.toml',ROOT/'Cargo.lock',ROOT/'runtime-version.json',Path(__file__),ROOT/'scripts/validate-desktop.py',ROOT/'scripts/validate-desktop-interop.py']
    for directory in ['crates','runtime/crates/trames-wayland']:
        sources.extend(p for p in (ROOT/directory).rglob('*') if p.is_file() and 'target' not in p.relative_to(ROOT).parts)
    sources.extend(ROOT/'tests/fixtures'/name for name in ['keyboard-client.c','keyboard-x11.c','keyboard-control.c','desktop-client.c','desktop-probe.c'])
    inputs=dict(binaries={k:m.digest(v) for k,v in frozen.items()},tools={k:m.digest(tools/k) for k in builds},runtime_pin=json.loads((ROOT/'runtime-version.json').read_text()),sources={str(p.relative_to(ROOT)):m.digest(p) for p in sorted(set(sources))},physical_gpu_qualified=False)
    (out/'inputs.json').write_text(json.dumps(inputs,indent=2,sort_keys=True)+'\n')
    rows=[]
    def case(name,host_wayland,guest,transport=None,cpus=1):
        if args.case and name not in args.case:return
        directory=out/name;directory.mkdir();process=None;scratch=None
        try:
            keyboard='[keyboard]\nkeymap_layout=de\nrepeat-rate=25\nrepeat-delay=150\n'
            with m.host(directory,host_wayland,tools,keyboard=keyboard,parent_keymap=['-layout','de']) as env:
                env=env|{'TRAMES_DESKTOP_HOST':'wayland' if host_wayland else 'x11','TRAMES_DESKTOP_CLIENT':str(tools/'wayland-client')}
                if guest=='reference':
                    argv=[tools/('wayland-client' if host_wayland else 'x11-client')]
                    x11=not host_wayland
                elif transport is None:
                    argv=[frozen['unit_binary'],'desktop::tests::native_wayland_desktop','--ignored','--exact','--nocapture'];x11=False
                else:
                    x11=guest=='xwayland';scratch=directory/'rootfs'
                    shutil.copytree(args.rootfs if x11 else root,scratch)
                    if x11:
                        m.desktop.install(tools/'x11-client','/test/keyboard',scratch,set())
                        for path in ['usr/share/X11/locale','usr/lib/locale/C.utf8']:shutil.copytree('/'+path,scratch/path,dirs_exist_ok=True)
                    (directory/'rootfs-inputs.json').write_text(json.dumps({str(p.relative_to(scratch)):m.digest(p) for p in scratch.rglob('*') if p.is_file()},indent=2,sort_keys=True)+'\n')
                    argv=[frozen['runner'],'run','--rootfs',scratch,'--desktop','wayland' if host_wayland else 'x11','--cpus',str(cpus),'--timeout-ms','60000']
                    if x11:
                        argv+=['--xwayland-launcher',frozen['launcher'],'--setenv','LC_ALL=C.UTF-8','--','/bin/sh','-ec','trames-xwayland :0 -ac >&2 & xpid=$!; trap "kill $xpid" EXIT; while [ ! -S /tmp/.X11-unix/X0 ]; do sleep .05; done; DISPLAY=:0 /test/keyboard']
                    else:argv+=['--','/test/keyboard']
                    env=env|{'TRAMES_KVM_TRANSPORT':transport}
                process=m.desktop.Process(argv,env,directory/'client')
                title='*' if host_wayland else m.desktop.TITLE
                process.expect(b'keymap q=71 y=7a')
                if not host_wayland:m.desktop.pixels(tools/'probe',env,title,b'pixels 240 160 20c040\n')
                m.desktop.probe(tools/'probe',env,title,'focus');process.expect(b'focus-in')
                def key(code,down):m.desktop.probe(tools/'probe',env,title,'key',code,down)
                def tap(code,text=None,sym=None):
                    start=len(process.data);key(code,1);key(code,0)
                    if text is not None:process.expect((f'physical {code-8} 1 sym={sym} text={text}').encode(),after=start)
                if not host_wayland:
                    subprocess.run([tools/'control','1'],env=env,check=True);tap(24,'Q','51');tap(24,'q','71')
                tap(29,'z','7a')
                key(108,1);tap(24,'@','40');key(108,0)
                tap(66);tap(24,'Q','51')
                # Change the actual parent XKB map while locked: rebuilt guest
                # XKB state must retain the lock through the replacement.
                start=len(process.data);subprocess.run(['setxkbmap','-layout','fr'],env=env,check=True);process.expect(b'keymap q=61 y=79',after=start);tap(24,'A','41');tap(66);tap(24,'a','61')
                tap(77);tap(87,'1','ffb1');tap(77)
                start=len(process.data);subprocess.run(['setxkbmap','-layout','us,ru','-option','grp:alt_shift_toggle'],env=env,check=True);process.expect(b'keymap q=71 y=79',after=start)
                key(64,1);key(50,1);key(50,0);key(64,0)
                if not x11:process.expect(b'state 0 0 0 1',after=start)
                tap(24,'й','6ca')
                # Restore one group before testing focus/physical repeat.
                start=len(process.data);subprocess.run(['setxkbmap','-layout','de','-option',''],env=env,check=True);process.expect(b'keymap q=71 y=7a',after=start)
                key(50,1);m.desktop.probe(tools/'probe',env,title,'unfocus');process.expect(b'focus-out',after=start);key(50,0);start=len(process.data);m.desktop.probe(tools/'probe',env,title,'focus');process.expect(b'focus-in',after=start);tap(24,'q','71')
                if not host_wayland:
                    subprocess.run(['xset','r','rate','150','25'],env=env,check=True)
                if not x11:process.expect(b'repeat 25 150')
                start=len(process.data);key(38,1)
                deadline=time.monotonic()+(3 if x11 else .5)
                while time.monotonic()<deadline:
                    process.drain(min(.1,deadline-time.monotonic()))
                    if x11 and process.data[start:].count(b'physical 30 1')>=3:break
                key(38,0);process.expect(b'physical 30 0',after=start)
                downs=process.data[start:].count(b'physical 30 1')
                assert (downs>=2 if x11 else downs==1),(name,downs,process.data[start:])
                process.child.stdin.write(b'q\n');process.child.stdin.flush();process.expect(b'keyboard-done');process.finish()
                rows.append(dict(id=name,status='passed',latched_shift=not host_wayland,german=True,french=True,russian_group=True,altgr=True,caps_across_map_change=True,numlock=True,focus_release=True,physical_down_events_during_hold=downs,repeat_policy='original-X11-server' if x11 else 'client-repeat-info'))
        except Exception as error:
            rows.append(dict(id=name,status='failed',reason=str(error)));raise
        finally:
            if process:process.close()
            if scratch:shutil.rmtree(scratch)
            (out/'results.json').write_text(json.dumps(dict(complete=False,passed=False,rows=rows),indent=2)+'\n')
        print(name+': passed',flush=True)
    for wayland in [False,True]:
        label='wayland' if wayland else 'x11'
        case('reference-'+label,wayland,'reference')
        case('native-'+label,wayland,'wayland')
    if not args.native_only:
        for transport in ['smccc','mmio']:
            for cpus in [1,4]:
                for wayland in [False,True]:
                    for guest in ['wayland','xwayland']:
                        case(f'kvm-{transport}-{cpus}-host-'+('wayland' if wayland else 'x11')+'-'+guest,wayland,guest,transport,cpus)
    assert rows
    if args.case:assert {r['id'] for r in rows}==set(args.case)
    (out/'results.json').write_text(json.dumps(dict(complete=True,passed=all(r['status']=='passed' for r in rows),rows=rows,physical_gpu_qualified=False),indent=2)+'\n')
    print(json.dumps(dict(passed=True,cases=len(rows))))


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['unit-binary','runner','receipt','rootfs','launcher','output']:p.add_argument('--'+name,type=Path,required=name in ['unit-binary','output'])
    p.add_argument('--native-only',action='store_true');p.add_argument('--case',action='append');a=p.parse_args()
    if not a.native_only and not all([a.runner,a.receipt,a.rootfs,a.launcher]):p.error('KVM needs --runner --receipt --rootfs --launcher')
    run(a)
