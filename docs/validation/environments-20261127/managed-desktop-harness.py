#!/usr/bin/env python3
"""Real named-environment desktop and clipboard grant acceptance."""
import importlib.util,json,os,shutil,subprocess,sys,tempfile
from pathlib import Path
root=Path('/mnt/trames-output/linux-keyboard-source');out=Path(sys.argv[1]);out.mkdir(exist_ok=False)
spec=importlib.util.spec_from_file_location('interop',root/'scripts/validate-desktop-interop.py');i=importlib.util.module_from_spec(spec);spec.loader.exec_module(i);d=i.desktop
runner=out/'runner';shutil.copy2('/mnt/trames-output/linux-desktop-target/release/trames',runner);runner.chmod(0o555)
tools=out/'tools';tools.mkdir()
for name in ['interop-client','input-method','xclip','probe']:
 shutil.copy2(Path('/mnt/trames-output/linux-interop/qualification-final/tools')/name,tools/name)
source=out/'rootfs';source.mkdir();copied=set()
for src,dst in [('/bin/sh','/bin/sh'),('/usr/bin/setsid','/usr/bin/setsid'),(tools/'interop-client','/test/interop')]:d.install(src,dst,source,copied)
for path in ['run','tmp','proc','dev','root','etc']:(source/path).mkdir(exist_ok=True)
rows=[]
for transport,cpus in [('smccc',1),('mmio',4)]:
 for wayland in [False,True]:
  for allow in [False,True]:
   label=f'{transport}-'+('wayland' if wayland else 'x11')+f'-clipboard-{allow}';case=out/label;case.mkdir()
   with i.host(case,wayland,tools) as env,tempfile.TemporaryDirectory(prefix='registry-',dir=case) as temp:
    if not wayland:env.pop('WAYLAND_DISPLAY',None)
    env|={'TRAMES_HOME':temp,'TRAMES_KVM_TRANSPORT':transport};commands=[]
    def run(*argv):
     r=subprocess.run([str(runner),*map(str,argv)],env=env,capture_output=True,timeout=70);commands.append(dict(argv=list(map(str,argv)),code=r.returncode,stdout=r.stdout.decode(errors='replace'),stderr=r.stderr.decode(errors='replace')));(case/'commands.json').write_text(json.dumps(commands,indent=2)+'\n');assert r.returncode==0,commands[-1];return r
    guest=owner=peer=None
    try:
     run('config','--global','set','defaults.components.vulkan','false')
     run('env','create','work','--rootfs',source,'--cpus',cpus,'--memory','auto')
     run('-e','work','config','set','interop.clipboard.enabled',str(allow).lower())
     if wayland:
      owner=d.Process([tools/'interop-client'],env,case/'owner');owner.expect(b'focus-in')
      for pressed in [1,0]:d.probe(tools/'probe',env,'*','key',40,pressed)
      owner.expect(b'key 32 0');i.send(owner,'value host-sentinel');i.send(owner,'copy');owner.expect(b'copy-ready')
     else:
      owner=d.Process([tools/'xclip','own','host-sentinel'],env,case/'owner');owner.expect(b'owner-ready')
     run('env','start','work')
     guest=d.Process([runner,'-e','work','exec','--','/test/interop'],env,case/'guest');guest.expect(b'size ')
     d.probe(tools/'probe',env,'*' if wayland else d.TITLE,'focus');guest.expect(b'focus-in')
     for pressed in [1,0]:d.probe(tools/'probe',env,'*' if wayland else d.TITLE,'key',40,pressed)
     guest.expect(b'key 32 0')
     # The original client processes input and renders through the named worker.
     if not allow:
      guest.drain(.5);assert b'selection 1' not in guest.data,guest.data
     i.send(guest,'value guest-sentinel');i.send(guest,'copy');guest.expect(b'copy-ready')
     if allow:guest.expect(b'clipboard-sent')
     if wayland:
      peer=d.Process([tools/'interop-client'],env,case/'peer');peer.expect(b'focus-in');peer.expect(b'selection 1');i.send(peer,'paste');peer.expect(b'paste '+(b'guest-sentinel' if allow else b'host-sentinel')+b'\n')
     else:
      value=subprocess.check_output([tools/'xclip','get'],env=env,timeout=15);assert value.strip()==(b'guest-sentinel' if allow else b'host-sentinel'),value
     i.send(guest,'quit');guest.expect(b'interop-done');guest.finish()
     if peer:i.send(peer,'quit');peer.expect(b'interop-done');peer.finish()
     rows.append(dict(id=label,passed=True,clipboard_enabled=allow,original_client_input=True,clipboard_host_value=True));(out/'results.json').write_text(json.dumps(dict(passed=True,complete=len(rows)==8,runner_sha256=d.digest(runner),tools_sha256={p.name:d.digest(p) for p in tools.iterdir()},rows=rows),indent=2)+'\n');print(label+': passed',flush=True)
    finally:
     for process in [guest,peer,owner]:
      if process:process.close()
     subprocess.run([str(runner),'env','stop','work'],env=env,capture_output=True,timeout=65)
shutil.rmtree(source)
