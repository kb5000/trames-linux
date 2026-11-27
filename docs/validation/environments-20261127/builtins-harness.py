import json,os,subprocess,tempfile,sys
from pathlib import Path
out=Path(sys.argv[1]);out.mkdir(exist_ok=False);runner=Path(sys.argv[2]);rows=[]
with tempfile.TemporaryDirectory(prefix='registry-',dir=out) as home:
 env=os.environ|{'TRAMES_HOME':home};env.pop('DISPLAY',None);env.pop('WAYLAND_DISPLAY',None)
 def run(*args,ok=True):
  p=subprocess.run([str(runner),*map(str,args)],env=env,capture_output=True,timeout=90);row=dict(argv=list(map(str,args)),code=p.returncode,stdout=p.stdout.decode(errors='replace'),stderr=p.stderr.decode(errors='replace'));rows.append(row);(out/'operations.json').write_text(json.dumps(rows,indent=2)+'\n');assert (p.returncode==0)==ok,row;return p
 run('config','--global','set','defaults.components.vulkan','false','defaults.display.windows','false','defaults.resources.cpus','2')
 try:
  for n,archive in [('alpine','/mnt/trames-output/vaapi-build/alpine-minirootfs-3.22.6-aarch64.tar.gz'),('debian','/mnt/trames-output/trames-mount-debian-rootfs.tar.gz')]:
   run('env','create',n,'--distro',n,'--archive',archive)
   r=run('-e',n,'exec','--','/bin/sh','-ec','echo named-image-ok; uname -m; cat /etc/os-release; echo durable > /root/value; sync');assert b'named-image-ok' in r.stdout and b'aarch64' in r.stdout
   run('env','stop',n)
   assert run('-e',n,'exec','--','/bin/cat','/root/value').stdout==b'durable\n'
   run('env','stop',n)
  run('env','create','bad-checksum','--archive','/mnt/trames-output/vaapi-build/alpine-minirootfs-3.22.6-aarch64.tar.gz','--sha256','0'*64,ok=False)
  (out/'results.json').write_text(json.dumps(dict(passed=True,operations=len(rows),rows=rows),indent=2)+'\n')
 finally:
  for n in ['alpine','debian']:subprocess.run([str(runner),'env','stop',n],env=env,capture_output=True,timeout=65)
print('builtin Alpine/Debian: passed')
