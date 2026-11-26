#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Original guest Xlib/XIM over real KVM, shared Xwayland and host XIM."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('interop',ROOT/'scripts/validate-desktop-interop.py')
interop=importlib.util.module_from_spec(spec)
spec.loader.exec_module(interop)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['runner','unit-binary','rootfs','launcher','output']:
        p.add_argument('--'+name,type=Path,required=True)
    args=p.parse_args()
    out=args.output.resolve()
    out.mkdir(parents=True,exist_ok=False)
    binaries={}
    for name in ['runner','unit_binary','launcher']:
        source=getattr(args,name).resolve()
        target=out/name
        shutil.copy2(source,target)
        target.chmod(0o555)
        assert digest(source)==digest(target)
        binaries[name]=target
    argv=[os.environ.get('CC','cc'),'-O2','-Wall','-Wextra','-Werror']
    argv+=shlex.split(os.environ.get('X11_CFLAGS',''))
    argv+=[str(ROOT/'tests/fixtures/interop-xim-client.c')]
    argv+=shlex.split(os.environ.get('X11_LIBS','-lX11'))
    argv+=['-o',str(out/'client')]
    (out/'client.command.json').write_text(json.dumps(argv,indent=2)+'\n')
    with (out/'client.build.log').open('wb') as log:
        subprocess.run(argv,check=True,stdout=log,stderr=subprocess.STDOUT)
    subprocess.run([os.environ.get('CC','cc'),'-O2','-Wall','-Wextra','-Werror',ROOT/'tests/fixtures/desktop-probe.c','-lxcb','-lxcb-xtest','-o',out/'probe'],check=True)
    control=[os.environ.get('CC','cc'),'-O2','-Wall','-Wextra','-Werror',str(ROOT/'tests/fixtures/xim-control.c'),'-lxcb','-lxcb-xtest','-o',str(out/'control')]
    (out/'control.command.json').write_text(json.dumps(control,indent=2)+'\n')
    with (out/'control.build.log').open('wb') as log:subprocess.run(control,check=True,stdout=log,stderr=subprocess.STDOUT)
    sources={str(path.relative_to(ROOT)):digest(path) for path in [Path(__file__),ROOT/'scripts/validate-desktop-interop.py',ROOT/'scripts/validate-desktop.py',ROOT/'tests/fixtures/interop-xim-client.c',ROOT/'tests/fixtures/desktop-probe.c',ROOT/'tests/fixtures/xim-control.c']}
    inputs=dict(binaries={k:digest(v) for k,v in binaries.items()},client_sha256=digest(out/'client'),control_sha256=digest(out/'control'),sources=sources,runtime_pin=json.loads((ROOT/'runtime-version.json').read_text()),physical_gpu_qualified=False)
    (out/'inputs.json').write_text(json.dumps(inputs,indent=2)+'\n')
    rows=[]
    for transport in ['smccc','mmio']:
        for cpus in [1,4]:
            name=f'xwayland-ime-{transport}-{cpus}'
            case=out/name
            case.mkdir()
            root=case/'rootfs'
            shutil.copytree(args.rootfs,root)
            interop.desktop.install(out/'client','/test/xim-client',root,set())
            shutil.copytree('/usr/share/X11/locale',root/'usr/share/X11/locale',dirs_exist_ok=True)
            shutil.copytree('/usr/lib/locale/C.utf8',root/'usr/lib/locale/C.utf8',dirs_exist_ok=True)
            (case/'rootfs-inputs.json').write_text(json.dumps({str(f.relative_to(root)):digest(f) for f in root.rglob('*') if f.is_file()},indent=2,sort_keys=True)+'\n')
            im=guest=None
            try:
                with interop.host(case,False,out) as env:
                    env=env|{'TRAMES_KVM_TRANSPORT':transport}
                    im=interop.desktop.Process([binaries['unit_binary'],'interop_tests::host_xim_peer','--ignored','--exact','--nocapture'],env,case/'im')
                    im.expect(b'xim-ready')
                    command='trames-xwayland :0 -ac >&2 & xpid=$!; trap "kill $xpid" EXIT; while [ ! -S /tmp/.X11-unix/X0 ]; do sleep .05; done; DISPLAY=:0 /test/xim-client'
                    guest=interop.desktop.Process([binaries['runner'],'run','--rootfs',root,'--desktop','x11','--xwayland-launcher',binaries['launcher'],'--setenv','LC_ALL=C.UTF-8','--cpus',str(cpus),'--timeout-ms','60000','--','/bin/sh','-ec',command],env,case/'guest')
                    guest.expect(b'guest-xim-ready')
                    interop.desktop.probe(out/'probe',env,'Trames XIM acceptance','focus')
                    im.expect(b'xim-focus')
                    # Enter can commit text and close the application before a
                    # separate key-up probe can find its window again.
                    subprocess.run([out/'control'],env=env,check=True)
                    guest.expect('guest-xim-commit 中文输入 ✅\n'.encode())
                    guest.finish()
                    rows.append(dict(id=name,status='passed',original_xlib=True,utf8_commit=True))
            except Exception as error:
                rows.append(dict(id=name,status='failed',reason=str(error)))
                raise
            finally:
                for process in [guest,im]:
                    if process:
                        process.close()
                shutil.rmtree(root)
                (out/'results.json').write_text(json.dumps(dict(complete=len(rows)==4,passed=all(r['status']=='passed' for r in rows),rows=rows),indent=2)+'\n')
            print(name+': passed',flush=True)


if __name__=='__main__':
    main()
