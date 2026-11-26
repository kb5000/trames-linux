#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Original xdg-shell cyclic-parent error oracle; native references and KVM."""
import argparse,importlib.util,json,os,resource,shutil,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('windows',ROOT/'scripts/validate-windows.py');w=importlib.util.module_from_spec(spec);spec.loader.exec_module(w);d=w.d

def main(a):
 resource.setrlimit(resource.RLIMIT_CORE,(0,0))
 out=a.output.resolve();out.mkdir(parents=True,exist_ok=False);tools=out/'tools';tools.mkdir();frozen={}
 for name in ['unit_binary','runner','baseline_unit']:
  source=getattr(a,name)
  if source:
   target=tools/name;shutil.copy2(source,target);target.chmod(0o555);assert d.digest(source)==d.digest(target);frozen[name]=target
 xml=Path(subprocess.check_output(['pkg-config','--variable=pkgdatadir','wayland-protocols'],text=True).strip())/'stable/xdg-shell/xdg-shell.xml'
 for mode,suffix in [('client-header','client-protocol.h'),('private-code','protocol.c')]:subprocess.run(['wayland-scanner',mode,xml,tools/('xdg-shell-'+suffix)],check=True)
 command=[os.environ.get('CC','cc'),'-O2','-Wall','-Wextra','-Werror','-I'+str(tools),str(ROOT/'tests/fixtures/window-parent-error.c'),str(tools/'xdg-shell-protocol.c'),'-lwayland-client','-o',str(tools/'peer')]
 (tools/'peer.command.json').write_text(json.dumps(command,indent=2)+'\n')
 with (tools/'peer.build.log').open('wb') as log:subprocess.run(command,check=True,stdout=log,stderr=subprocess.STDOUT)
 sources=[Path(__file__),ROOT/'scripts/validate-windows.py',ROOT/'scripts/validate-desktop.py',ROOT/'tests/fixtures/window-parent-error.c',ROOT/'tests/fixtures/window-client.c',ROOT/'runtime-version.json']
 sources+=list((ROOT/'runtime/crates/trames-wayland/src').glob('*.rs'))
 inputs=dict(binaries={k:d.digest(v) for k,v in frozen.items()},peer_sha256=d.digest(tools/'peer'),protocol_sha256=d.digest(xml),runtime_pin=json.loads((ROOT/'runtime-version.json').read_text()),sources={str(p.relative_to(ROOT)):d.digest(p) for p in sources})
 (out/'inputs.json').write_text(json.dumps(inputs,indent=2,sort_keys=True)+'\n')
 if a.runner:
  (out/'doctor.json').write_bytes(subprocess.check_output([frozen['runner'],'doctor','--json','--require-kvm']))
  receipt=json.loads(a.receipt.read_text());assert receipt['fingerprint']['binary_sha256']==d.digest(frozen['runner']);shutil.copy2(a.receipt,out/'build-receipt.json')
 rows=[];baseline=None
 def save(complete):
  (out/'results.json').write_text(json.dumps(dict(complete=complete,passed=complete and all(r['status']=='passed' for r in rows),rows=rows,baseline=baseline,physical_gpu_qualified=False),indent=2)+'\n')
 def case(name,wayland,kind,transport=None,cpus=1,old=False):
  nonlocal baseline
  path=out/name;path.mkdir();process=None;scratch=None
  try:
   with w.host(path,wayland,None) as env:
    env=env|{'TRAMES_DESKTOP_HOST':'wayland' if wayland else 'x11','TRAMES_DESKTOP_CLIENT':str(tools/'peer')}
    if kind=='reference':argv=[tools/'peer']
    elif kind=='native':argv=[frozen['baseline_unit' if old else 'unit_binary'],'desktop::tests::native_wayland_desktop','--ignored','--exact','--nocapture']
    else:
     scratch=path/'rootfs';scratch.mkdir()
     for p in ['tmp','run','root','proc','dev','usr/bin']:(scratch/p).mkdir(parents=True,exist_ok=True)
     (scratch/'tmp').chmod(0o1777);d.install(tools/'peer','/test/parent-error',scratch,set())
     (path/'rootfs-inputs.json').write_text(json.dumps({str(p.relative_to(scratch)):d.digest(p) for p in scratch.rglob('*') if p.is_file()},indent=2,sort_keys=True)+'\n')
     argv=[frozen['runner'],'run','--rootfs',scratch,'--desktop','wayland' if wayland else 'x11','--cpus',str(cpus),'--timeout-ms','30000','--','/test/parent-error'];env=env|{'TRAMES_KVM_TRANSPORT':transport}
    process=d.Process(argv,env,path/'client');process.expect(('parent-error code='+('3' if old else '1')+' interface=xdg_toplevel').encode(),seconds=15)
    if old:
     process.child.wait(timeout=10);process.drain();assert process.child.returncode!=0;baseline=dict(id=name,status='failed-as-expected',incorrect_error=3,return_code=process.child.returncode)
    else:
     process.expect(b'parent-error-done',seconds=10);process.finish();rows.append(dict(id=name,status='passed',protocol_error=1,interface='xdg_toplevel'))
  except Exception as error:
   if not old:rows.append(dict(id=name,status='failed',reason=str(error)))
   raise
  finally:
   if process:process.close()
   if scratch:shutil.rmtree(scratch)
   save(False)
  print(name+(': failed as expected (old code 3)' if old else ': passed'),flush=True)
 if a.baseline_unit:case('baseline-invalid-code',False,'native',old=True)
 for wayland in [False,True]:case('native-'+('wayland' if wayland else 'x11'),wayland,'native')
 if not a.native_only:
  for transport,cpus in [('smccc',1),('mmio',4)]:
   for wayland in [False,True]:case(f'kvm-{transport}-{cpus}-'+('wayland' if wayland else 'x11'),wayland,'kvm',transport,cpus)
 save(True);print(json.dumps(dict(passed=True,cases=len(rows),baseline_reproduced=baseline is not None)))
if __name__=='__main__':
 p=argparse.ArgumentParser(description=__doc__)
 for name in ['unit-binary','runner','baseline-unit','receipt','output']:p.add_argument('--'+name,type=Path,required=name in ['unit-binary','output'])
 p.add_argument('--native-only',action='store_true');a=p.parse_args()
 if not a.native_only and not (a.runner and a.receipt):p.error('KVM requires --runner and --receipt')
 main(a)
