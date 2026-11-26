#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Original native WM state/geometry/gesture acceptance and real ARM64 KVM."""
import argparse,contextlib,importlib.util,json,os,re,resource,shlex,shutil,signal,subprocess,tempfile,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
_spec=importlib.util.spec_from_file_location('desktop',ROOT/'scripts/validate-desktop.py');d=importlib.util.module_from_spec(_spec);_spec.loader.exec_module(d)
TITLE='Trames window parent'
@contextlib.contextmanager
def host(case,wayland,probe):
 with d.xvfb(case) as display,tempfile.TemporaryDirectory(prefix='trames-windows-') as runtime:
  env=os.environ|{'DISPLAY':display,'XDG_RUNTIME_DIR':runtime,'WAYLAND_DISPLAY':'windows','XMODIFIERS':'@im=none'}
  if wayland:
   config=case/'weston.ini';config.write_text('[core]\nidle-time=0\n[input-method]\npath=/nonexistent/trames-input-method\n')
   argv=['weston','--backend=x11','--renderer=pixman','--shell=desktop-shell.so','--width=800','--height=600','--socket=windows','--config='+str(config)]
  else:
   config=case/'openbox.xml';config.write_text('<openbox_config xmlns="http://openbox.org/3.4/rc"><theme><name>Onyx</name></theme><focus><focusNew>yes</focusNew></focus><keyboard><keybind key="A-Tab"><action name="NextWindow"/></keybind></keyboard></openbox_config>')
   argv=[os.environ.get('OPENBOX','openbox'),'--debug','--config-file',str(config),'--startup',shlex.join(['/usr/bin/touch',str(case/'wm-ready')])]
  (case/'host.command.json').write_text(json.dumps(argv,indent=2)+'\n')
  with (case/'host.log').open('wb') as log:
   child=subprocess.Popen(argv,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
   try:
    deadline=time.monotonic()+15
    while True:
     ready=Path(runtime,'windows').exists() if wayland else (case/'wm-ready').exists()
     if ready:break
     if child.poll() is not None or time.monotonic()>deadline:raise RuntimeError('native WM startup failed')
     time.sleep(.02)
    yield env
   finally:
    if child.poll() is None:os.killpg(child.pid,signal.SIGTERM)
    child.wait(timeout=10)

def main(a):
 resource.setrlimit(resource.RLIMIT_CORE,(0,0))
 out=a.output.resolve();out.mkdir(parents=True,exist_ok=False);tools=out/'tools';tools.mkdir();frozen={}
 for name in ['unit_binary','runner','launcher']:
  source=getattr(a,name)
  if source:
   target=tools/name;shutil.copy2(source,target);target.chmod(0o555);assert d.digest(source)==d.digest(target);frozen[name]=target
 xml=Path(subprocess.check_output(['pkg-config','--variable=pkgdatadir','wayland-protocols'],text=True).strip())/'stable/xdg-shell/xdg-shell.xml'
 for mode,suffix in [('client-header','client-protocol.h'),('private-code','protocol.c')]:subprocess.run(['wayland-scanner',mode,xml,tools/('xdg-shell-'+suffix)],check=True)
 cc=[os.environ.get('CC','cc'),'-O2','-Wall','-Wextra','-Werror']
 builds={'wayland-client':cc+['-I'+str(tools),ROOT/'tests/fixtures/window-client.c',tools/'xdg-shell-protocol.c','-lwayland-client'], 'x11-client':cc+shlex.split(os.environ.get('X11_CFLAGS',''))+[ROOT/'tests/fixtures/window-x11.c']+shlex.split(os.environ.get('X11_LIBS','-lX11')), 'probe':cc+[ROOT/'tests/fixtures/window-probe.c','-lxcb','-lxcb-xtest']}
 for name,argv in builds.items():
  argv=[str(v) for v in argv+['-o',tools/name]];(tools/(name+'.command.json')).write_text(json.dumps(argv,indent=2)+'\n')
  with (tools/(name+'.build.log')).open('wb') as log:subprocess.run(argv,check=True,stdout=log,stderr=subprocess.STDOUT)
 sources=[ROOT/'Cargo.toml',ROOT/'Cargo.lock',ROOT/'runtime-version.json',Path(__file__),ROOT/'scripts/validate-desktop.py']
 for directory in ['crates','runtime/crates/trames-wayland']:
  sources.extend(p for p in (ROOT/directory).rglob('*') if p.is_file() and 'target' not in p.relative_to(ROOT).parts)
 sources.extend(ROOT/'tests/fixtures'/name for name in ['window-client.c','window-x11.c','window-probe.c'])
 (out/'inputs.json').write_text(json.dumps(dict(binaries={k:d.digest(v) for k,v in frozen.items()},tools={k:d.digest(tools/k) for k in builds},runtime_pin=json.loads((ROOT/'runtime-version.json').read_text()),sources={str(p.relative_to(ROOT)):d.digest(p) for p in sorted(set(sources))}),indent=2,sort_keys=True)+'\n')
 if a.runner:
  (out/'doctor.json').write_bytes(subprocess.check_output([frozen['runner'],'doctor','--json','--require-kvm']))
  receipt=json.loads(a.receipt.read_text());assert receipt['fingerprint']['binary_sha256']==d.digest(frozen['runner']);shutil.copy2(a.receipt,out/'build-receipt.json')
 rows=[]
 def case(name,wayland,guest,transport=None,cpus=1):
  if a.case and name not in a.case:return
  directory=out/name;directory.mkdir();process=None;scratch=None
  try:
   with host(directory,wayland,tools/'probe') as env:
    env=env|{'TRAMES_DESKTOP_HOST':'wayland' if wayland else 'x11','TRAMES_DESKTOP_CLIENT':str(tools/'wayland-client')}
    if guest=='reference':argv=[tools/('wayland-client' if wayland else 'x11-client')]
    elif transport is None:argv=[frozen['unit_binary'],'desktop::tests::native_wayland_desktop','--ignored','--exact','--nocapture']
    else:
     scratch=directory/'rootfs'
     if guest=='xwayland':shutil.copytree(a.rootfs,scratch)
     else:
      scratch.mkdir()
      for path in ['tmp','run','root','proc','dev','usr/bin']:(scratch/path).mkdir(parents=True,exist_ok=True)
      (scratch/'tmp').chmod(0o1777)
     d.install(tools/('x11-client' if guest=='xwayland' else 'wayland-client'),'/test/window',scratch,set())
     (directory/'rootfs-inputs.json').write_text(json.dumps({str(p.relative_to(scratch)):d.digest(p) for p in scratch.rglob('*') if p.is_file()},indent=2,sort_keys=True)+'\n')
     argv=[frozen['runner'],'run','--rootfs',scratch,'--desktop','wayland' if wayland else 'x11','--cpus',str(cpus),'--timeout-ms','60000']
     if guest=='xwayland':argv+=['--xwayland-launcher',frozen['launcher'],'--','/bin/sh','-ec','trames-xwayland :0 -ac >&2 & xpid=$!; trap "kill $xpid" EXIT; while [ ! -S /tmp/.X11-unix/X0 ]; do sleep .05; done; DISPLAY=:0 /test/window']
     else:argv+=['--','/test/window']
     env=env|{'TRAMES_KVM_TRANSPORT':transport}
    process=d.Process(argv,env,directory/'client');title='*' if wayland else TITLE
    def probe(mode,*args):
     value=d.probe(tools/'probe',env,title,mode,*args)
     with (directory/'probe-observations.jsonl').open('a') as log:log.write(json.dumps(dict(mode=mode,args=args,value=value.decode()))+'\n')
     return value
    def send(c):
     at=len(process.data);process.child.stdin.write(c.encode());process.child.stdin.flush();process.expect(('command '+c).encode(),after=at);return at
    def configured(maximum,full,at=0,size=None):
     deadline=time.monotonic()+15
     while True:
      process.drain(.03)
      matches=list(re.finditer(rb'configured parent (\d+) (\d+) max=(\d) full=(\d)',process.data[at:]))
      for match in reversed(matches):
       width,height,m,f=map(int,match.groups())
       if (m,f)==(maximum,full) and (size is None or (width,height)==size) and ((not maximum or width>=640) and (not full or width>640)):return width,height
      if process.child.poll() is not None or time.monotonic()>deadline:raise RuntimeError('missing configured state '+str((maximum,full,size))+'; '+repr(process.data[-700:]))
    def eventually(fn,description):
     deadline=time.monotonic()+8
     while True:
      try:
       value=fn()
       if value:return value
      except subprocess.CalledProcessError:pass
      if process.child.poll() is not None or time.monotonic()>deadline:raise RuntimeError(description)
      process.drain(.03)
    def geometry():return tuple(map(int,probe('rect' if wayland else 'geometry').split()))
    configured(0,0,size=(240,160));eventually(geometry,'initial visible window');probe('focus');process.expect(b'focus parent')
    states=[]
    at=send('m');states.append(configured(1,0,at));at=send('n');configured(0,0,at,(240,160))
    at=send('f');states.append(configured(0,1,at));at=send('u');configured(0,0,at,(240,160))
    at=send('m');configured(1,0,at);at=send('f');eventually(lambda: b'full=1' in process.data[at:],'fullscreen over maximized')
    at=send('u');configured(1,0,at);at=send('n');configured(0,0,at,(240,160))
    pixel_size=(240,160) if guest=='xwayland' or (guest=='reference' and not wayland) else (272,192)
    eventually(lambda: geometry()[2:]==pixel_size,'floating pixels have not reached the confirmed geometry')
    def drag(command,resize):
     before=eventually(geometry,'visible pre-gesture geometry');x,y,w,h=before;at=send(command);probe('point',x+w//2,y+h//2);probe('button',1,1);process.expect(('gesture '+command).encode(),after=at)
     try:
      deadline=time.monotonic()+4;delta=10
      while True:
       probe('point',x+w//2+delta,y+h//2+delta);process.drain(.04);after=geometry()
       if (after[2]>w+5 and after[3]>h+5) if resize else (abs(after[0]-x)>5 or abs(after[1]-y)>5):break
       if time.monotonic()>deadline:raise RuntimeError('native gesture did not change geometry '+str((command,before,after)))
       delta=min(delta+10,100)
     finally:probe('button',1,0)
     return dict(before=before,after=after)
    move=drag('v',False);resize=drag('r',True)
    at=send('i');process.expect(b'blur parent',after=at)
    if wayland:
     eventually(lambda: subprocess.run([tools/'probe',title,'rect'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode!=0,'minimized host surface still visible')
     probe('focus')
     for key,pressed in [(133,1),(23,1),(23,0),(133,0)]:probe('key',key,pressed)
    else:
     eventually(lambda:probe('state').split()[2]==b'1','host did not minimize')
     probe('restore')
    process.expect(b'focus parent',after=at);eventually(geometry,'window did not restore')
    at=send('c');process.expect(b'configured child',after=at);process.expect(b'focus child',after=at)
    if wayland:eventually(lambda: probe('rect-red'),'child pixels missing')
    if not wayland:
     parent_id=int(probe('id'));eventually(lambda:int(d.probe(tools/'probe',env,'Trames window child','parent'))==parent_id,'transient parent missing')
    at=send('d')
    if not wayland:eventually(lambda:int(d.probe(tools/'probe',env,'Trames window child','parent'))==0,'transient parent did not detach')
    send('x')
    if wayland:eventually(lambda:subprocess.run([tools/'probe',title,'rect-red'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode!=0,'child pixels did not disappear')
    probe('focus');x,y,w,h=eventually(geometry,'parent pixels after child close');probe('point',x+w//2,y+h//2);probe('button',1,1);probe('button',1,0);process.expect(b'focus parent',after=at)
    send('s');eventually(lambda:geometry()[2:]==(pixel_size[0]+60,pixel_size[1]+40),'guest initiated resize did not reach native window')
    send('q');process.expect(b'window-done');process.finish()
    rows.append(dict(id=name,status='passed',maximized_and_fullscreen_distinct=True,restore_size=True,full_over_maximized=True,minimize_restore=True,parent_and_detach=True,focus=True,guest_resize=True,move=move,resize=resize,state_sizes=states,physical_gpu_qualified=False))
  except Exception as error:rows.append(dict(id=name,status='failed',reason=str(error)));raise
  finally:
   if process:process.close()
   if scratch:shutil.rmtree(scratch)
   (out/'results.json').write_text(json.dumps(dict(complete=False,passed=False,rows=rows),indent=2)+'\n')
  print(name+': passed',flush=True)
 for wayland in [False,True]:
  label='wayland' if wayland else 'x11';case('reference-'+label,wayland,'reference');case('native-'+label,wayland,'wayland')
 if not a.native_only:
  for transport in ['smccc','mmio']:
   for cpus in [1,4]:
    for wayland in [False,True]:
     for guest in ['wayland','xwayland']:case(f'kvm-{transport}-{cpus}-host-'+('wayland' if wayland else 'x11')+'-'+guest,wayland,guest,transport,cpus)
 assert rows
 if a.case:assert {r['id'] for r in rows}==set(a.case)
 (out/'results.json').write_text(json.dumps(dict(complete=True,passed=True,rows=rows,physical_gpu_qualified=False),indent=2)+'\n');print(json.dumps(dict(passed=True,cases=len(rows))))
if __name__=='__main__':
 p=argparse.ArgumentParser(description=__doc__)
 for name in ['unit-binary','runner','receipt','rootfs','launcher','output']:p.add_argument('--'+name,type=Path,required=name in ['unit-binary','output'])
 p.add_argument('--native-only',action='store_true');p.add_argument('--case',action='append');a=p.parse_args()
 if not a.native_only and not all([a.runner,a.receipt,a.rootfs,a.launcher]):p.error('KVM requires runner, receipt, rootfs and launcher')
 main(a)
