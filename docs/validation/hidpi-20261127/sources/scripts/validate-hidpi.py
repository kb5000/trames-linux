#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Real native display metadata, hotplug, fractional buffers and pixel/input acceptance."""
import argparse, contextlib, importlib.util, json, os, re, resource, shlex, shutil, signal, subprocess, tempfile, time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('desktop',ROOT/'scripts/validate-desktop.py');d=importlib.util.module_from_spec(spec);spec.loader.exec_module(d)
TITLE='Trames HiDPI'
def run(argv,env,**kw):return subprocess.check_output(list(map(str,argv)),env=env,**kw)
def eventually(fn,description,p=None):
 end=time.monotonic()+10
 while True:
  try:
   v=fn()
   if v:return v
  except (subprocess.CalledProcessError,FileNotFoundError,AssertionError):pass
  if p:
   p.drain(.02)
   if p.child.poll() is not None:raise RuntimeError(description+'; client exited; see '+str(p.path/'stderr'))
  else:time.sleep(.02)
  if time.monotonic()>end:raise RuntimeError(description)
@contextlib.contextmanager
def host(case,kind):
 with tempfile.TemporaryDirectory(prefix='trames-hidpi-') as runtime:
  env=os.environ|{'XDG_RUNTIME_DIR':runtime,'XMODIFIERS':'@im=none'}
  if kind=='wayland':
   config=case/'sway.conf';config.write_text('xwayland disable\noutput HEADLESS-1 mode 1280x960 scale 1 pos 0 0\noutput HEADLESS-2 mode 1600x1200 scale 1.25 pos 1280 0\nseat seat0 fallback true\ndefault_border none\ndefault_floating_border none\nfor_window [title="Trames HiDPI"] floating enable, move to output HEADLESS-1\nfocus_follows_mouse no\n')
   env|={'WLR_BACKENDS':'headless','WLR_RENDERER':'pixman','WLR_HEADLESS_OUTPUTS':'2','WLR_LIBINPUT_NO_DEVICES':'1'}
   argv=[os.environ.get('SWAY','sway'),'-d','-c',str(config)]
   with (case/'host.log').open('wb') as log:
    child=subprocess.Popen(argv,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
    try:
     sock=eventually(lambda:next(iter(Path(runtime).glob('sway-ipc.*.sock')),None),'Sway IPC unavailable');env['SWAYSOCK']=str(sock)
     env['WAYLAND_DISPLAY']=eventually(lambda:next((p.name for p in Path(runtime).glob('wayland-*') if not p.name.endswith('.lock')),None),'Wayland socket unavailable')
     pointer=subprocess.Popen([str(case.parent/'tools/pointer')],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE)
     assert pointer.stdout.readline()==b'pointer-ready\n'
     try:yield env
     finally:pointer.stdin.close();pointer.wait(timeout=5);pointer.stdout.close()
    finally:
     if child.poll() is None:os.killpg(child.pid,signal.SIGTERM)
     child.wait(timeout=10)
  else:
   with d.xvfb(case) as display:
    env['DISPLAY']=display
    # Xvfb's 1024px root is split into two real RandR 1.5 monitor objects.
    xr=os.environ.get('XRANDR','xrandr')
    run([xr,'--setmonitor','LEFT','512/135x768/203+0+0','screen'],env);run([xr,'--setmonitor','RIGHT','512/135x768/203+512+0','none'],env)
    env['TRAMES_DESKTOP_OUTPUT_SCALES']='LEFT=1,RIGHT=1.25'
    (case/'randr.txt').write_bytes(run([xr,'--listmonitors'],env))
    argv=[os.environ.get('OPENBOX','openbox'),'--sm-disable']
    with (case/'host.log').open('wb') as log:
     child=subprocess.Popen(argv,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
     try:
      eventually(lambda:subprocess.run([str(case.parent/'tools/probe'),TITLE,'wm'],env=env,stdout=subprocess.DEVNULL).returncode==0,'WM unavailable')
      yield env
     finally:
      if child.poll() is None:os.killpg(child.pid,signal.SIGTERM)
      child.wait(timeout=10)

def main(a):
 resource.setrlimit(resource.RLIMIT_CORE,(0,0));out=a.output.resolve();out.mkdir(parents=True,exist_ok=False);tools=out/'tools';tools.mkdir();frozen={}
 for name in ['unit_binary','runner']:
  src=getattr(a,name)
  if src:shutil.copy2(src,tools/name);frozen[name]=tools/name
 protocols=Path(subprocess.check_output(['pkg-config','--variable=pkgdatadir','wayland-protocols'],text=True).strip())
 xmls={'xdg-shell':protocols/'stable/xdg-shell/xdg-shell.xml','viewporter':protocols/'stable/viewporter/viewporter.xml','fractional-scale-v1':protocols/'staging/fractional-scale/fractional-scale-v1.xml','xdg-output-unstable-v1':protocols/'unstable/xdg-output/xdg-output-unstable-v1.xml','wlr-virtual-pointer-unstable-v1':ROOT/'tests/fixtures/wlr-virtual-pointer-unstable-v1.xml'}
 for name,xml in xmls.items():
  for mode,suffix in [('client-header','client-protocol.h'),('private-code','protocol.c')]:subprocess.run(['wayland-scanner',mode,xml,tools/(name+'-'+suffix)],check=True)
 cc=[os.environ.get('CC','cc'),'-O2','-Wall','-Wextra','-Werror']
 subprocess.run(cc+['-I'+str(tools),ROOT/'tests/fixtures/hidpi-client.c']+[p for p in tools.glob('*-protocol.c') if not p.name.startswith('wlr-')]+['-lwayland-client','-o',tools/'client'],check=True)
 subprocess.run(cc+['-I'+str(tools),ROOT/'tests/fixtures/hidpi-pointer.c',tools/'wlr-virtual-pointer-unstable-v1-protocol.c','-lwayland-client','-o',tools/'pointer'],check=True)
 subprocess.run(cc+[ROOT/'tests/fixtures/hidpi-probe.c','-lxcb','-lxcb-xtest','-o',tools/'probe'],check=True)
 if a.runner:
  assert a.receipt, 'KVM requires --receipt from acceptance/build.py'
  receipt=json.loads(a.receipt.read_text());assert receipt['fingerprint']['binary_sha256']==d.digest(frozen['runner'])
  shutil.copy2(a.receipt,out/'build-receipt.json')
  (out/'doctor.json').write_bytes(run([frozen['runner'],'doctor','--json','--require-kvm'],os.environ))
 rows=[]
 def case(name,kind,renderer='software',transport=None,cpus=1,reference=False):
  if a.case and name not in a.case:return
  c=out/name;c.mkdir();p=None;scratch=None
  try:
   with host(c,kind) as env:
    env|={'TRAMES_DESKTOP_HOST':kind,'TRAMES_DESKTOP_CLIENT':str(tools/'client'),'TRAMES_DESKTOP_RENDERER':renderer}
    if reference:argv=[tools/'client']
    elif transport:
     scratch=c/'rootfs';scratch.mkdir()
     for x in ['tmp','run','root','proc','dev','usr/bin']:(scratch/x).mkdir(parents=True,exist_ok=True)
     d.install(tools/'client','/test/hidpi',scratch,set());env['TRAMES_KVM_TRANSPORT']=transport
     argv=[frozen['runner'],'run','--rootfs',scratch,'--desktop',kind,'--desktop-renderer',renderer,'--cpus',str(cpus),'--timeout-ms','60000','--','/test/hidpi']
    else:argv=[frozen['unit_binary'],'desktop::tests::native_wayland_desktop','--ignored','--exact','--nocapture']
    p=d.Process(argv,env,c/'client');observations=[]
    def expect(marker,at=0):p.expect(marker.encode(),after=at)
    def send(char):at=len(p.data);p.child.stdin.write(char.encode());p.child.stdin.flush();expect('command '+char,at);return at
    def msg(command):
     value=json.loads(run([os.environ.get('SWAYMSG','swaymsg'),'-r',command],env))
     with (c/'commands.jsonl').open('a') as f:f.write(json.dumps(dict(command=command,response=value))+'\n')
     assert all(v['success'] for v in value),value
    def screenshot(output,width):
     # Read actual pixels from the unmodified compositor, not Runtime state.
     f=c/'capture.ppm';run([os.environ.get('GRIM','grim'),'-t','ppm','-o',output,f],env)
     data=f.read_bytes();head=re.match(rb'P6\s+(\d+)\s+(\d+)\s+255\s',data);assert head
     w,h=map(int,head.groups());b=data[head.end():];best=0
     for y in range(h):
      row=b[y*w*3:(y+1)*w*3];n=0;old=None
      for x in range(w):
       rgb=row[x*3:x*3+3];n=(n+1 if rgb!=old else 1) if rgb in [b'\x20\xc0\x40',b'\xc0\x40\x20'] else 0;best=max(best,n);old=rgb
      if best>=width:break
     if best<width-2:return False
     observations.append(dict(output=output,pixels=(w,h),alternating_run=best));f.unlink();return True
    expect('paint logical=240,160')
    if kind=='wayland':
     expect('HEADLESS-1');expect('HEADLESS-2');eventually(lambda:screenshot('HEADLESS-1',240),'1x screenshot mismatch',p)
     at=len(p.data);msg('[title="'+TITLE+'"] move container to output HEADLESS-2');expect('scale 150',at);expect('pixels=300,200 scale=150',at);eventually(lambda:screenshot('HEADLESS-2',300),'125% pixels are not sharp',p)
     # Native pointer coordinates remain logical after changing buffer density.
     msg('seat seat0 cursor set 1920 480');expect('pointer 120.00 80.00')
     at=len(p.data);msg('output HEADLESS-2 scale 1.5');expect('scale 180',at);expect('pixels=360,240 scale=180',at);eventually(lambda:screenshot('HEADLESS-2',360),'150% pixels are not sharp',p)
     at=len(p.data);msg('[title="'+TITLE+'"] move container to output HEADLESS-1');expect('scale 120',at)
     at=send('f');expect('full=1',at)
     def fullscreen_target():
      tree=json.loads(run([os.environ.get('SWAYMSG','swaymsg'),'-r','-t','get_tree'],env))
      def walk(node,output=None):
       if node.get('type')=='output':output=node['name']
       if node.get('name')==TITLE and node.get('fullscreen_mode')==1:return output
       return next((v for child in node.get('nodes',[])+node.get('floating_nodes',[]) if (v:=walk(child,output))),None)
      target=walk(tree)
      if target!='HEADLESS-2':return False
      (c/'fullscreen-target.json').write_text(json.dumps(dict(output=target,tree=tree),indent=2)+'\n');return True
     eventually(fullscreen_target,'explicit fullscreen did not select the other screen',p)
     at=send('u');expect('paint logical=240,160',at);msg('[title="'+TITLE+'"] move container to output HEADLESS-2')
     at=len(p.data);msg('output HEADLESS-2 disable');expect('removed HEADLESS-2',at)
     if not reference:expect('scale 120',at);expect('pixels=240,160 scale=120',at)
     at=len(p.data);msg('output HEADLESS-2 enable scale 2');expect('HEADLESS-2',at);msg('[title="'+TITLE+'"] move container to output HEADLESS-2');expect('scale 240',at);expect('pixels=480,320 scale=240',at);eventually(lambda:screenshot('HEADLESS-2',480),'2x pixels are not sharp',p)
     at=len(p.data);msg('output HEADLESS-2 transform 90');expect('transform=3',at)
    else:
     probe=lambda mode,*args:d.probe(tools/'probe',env,TITLE,mode,*args)
     expect('LEFT');expect('RIGHT')
     probe('move',40,100)
     eventually(lambda:tuple(map(int,probe('stripes').split()))==(240,160,240),'1x native pixels wrong',p)
     at=len(p.data);probe('move',600,100);expect('scale 150',at);expect('pixels=300,200 scale=150',at)
     eventually(lambda:probe('hints').split()==[b'200',b'125',b'800',b'600'],'scaled WM size hints wrong',p)
     eventually(lambda:tuple(map(int,probe('stripes').split()))==(300,200,300),'125% native pixels are not sharp',p)
     probe('motion',150,100);expect('pointer 120.00 80.00')
     at=len(p.data);probe('resize',400,250);expect('paint logical=320,200 pixels=400,250',at)
     at=len(p.data);probe('move',40,100);expect('scale 120',at)
     at=len(p.data);run([os.environ.get('XRANDR','xrandr'),'--delmonitor','RIGHT'],env);expect('removed RIGHT',at)
    send('q');expect('hidpi-done');p.finish();rows.append(dict(id=name,status='passed',host=kind,renderer=renderer,transport=transport,cpus=cpus,screenshots=observations,physical_gpu_qualified=False))
  except Exception as error:
   rows.append(dict(id=name,status='failed',reason=str(error)));raise
  finally:
   if p:p.close()
   if scratch:shutil.rmtree(scratch)
   (out/'results.json').write_text(json.dumps(dict(complete=False,rows=rows),indent=2)+'\n')
  print(name+': passed',flush=True)
 for kind in ['wayland','x11']:
  if kind=='wayland':case('reference-wayland',kind,reference=True)
  for renderer in ['software','vulkan']:case('native-'+kind+'-'+renderer,kind,renderer)
 if a.runner:
  for transport,cpus in [(t,c) for t in ['smccc','mmio'] for c in [1,4]]:
   for kind in ['wayland','x11']:case(f'kvm-{transport}-{cpus}-{kind}',kind,transport=transport,cpus=cpus)
 assert rows
 if a.case:assert set(a.case)=={v['id'] for v in rows}
 inputs=[ROOT/'Cargo.toml',ROOT/'Cargo.lock',Path(__file__),ROOT/'tests/fixtures/hidpi-client.c',ROOT/'tests/fixtures/hidpi-probe.c',ROOT/'tests/fixtures/hidpi-pointer.c',ROOT/'tests/fixtures/wlr-virtual-pointer-unstable-v1.xml']
 for directory in ['crates','runtime/crates/trames-wayland']:inputs.extend(p for p in (ROOT/directory).rglob('*') if p.is_file())
 (out/'inputs.json').write_text(json.dumps(dict(sources={str(p.relative_to(ROOT)):d.digest(p) for p in inputs},binaries={k:d.digest(v) for k,v in frozen.items()},tools={p.name:d.digest(p) for p in tools.iterdir() if p.is_file()}),indent=2,sort_keys=True)+'\n')
 (out/'results.json').write_text(json.dumps(dict(complete=True,passed=True,rows=rows),indent=2)+'\n')
if __name__=='__main__':
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--unit-binary',type=Path,required=True);p.add_argument('--runner',type=Path);p.add_argument('--receipt',type=Path);p.add_argument('--output',type=Path,required=True);p.add_argument('--case',action='append');main(p.parse_args())
