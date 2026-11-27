#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Disposable named-environment acceptance; never reads or modifies user registries."""
import argparse,threading,concurrent.futures,fcntl,hashlib,importlib.util,json,os,pty,resource,select,shutil,signal,socket,struct,subprocess,tarfile,tempfile,time,io
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('desktop',ROOT/'scripts/validate-desktop.py');d=importlib.util.module_from_spec(spec);spec.loader.exec_module(d)
def main(a):
 resource.setrlimit(resource.RLIMIT_CORE,(0,0));out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
 tools=out/'tools';tools.mkdir();runner=tools/'runner';shutil.copy2(a.runner,runner);runner.chmod(0o555)
 if not a.native_only:
  assert a.receipt, 'KVM requires --receipt from acceptance/build.py'
  receipt=json.loads(a.receipt.read_text());assert receipt['fingerprint']['binary_sha256']==d.digest(runner)
  shutil.copy2(a.receipt,out/'build-receipt.json')
  (out/'doctor.json').write_bytes(subprocess.check_output([str(runner),'doctor','--json','--require-kvm']))
 shutil.copy2(__file__,out/'harness.py')
 rows=[];log_lock=threading.Lock()
 env=dict(os.environ)
 for key in ['DISPLAY','WAYLAND_DISPLAY','TRAMES_HOME']:env.pop(key,None)
 def operation(home,*args,ok=True,data=None,extra=None):
  at=time.monotonic();r=subprocess.run([str(runner),*map(str,args)],env=env|{'TRAMES_HOME':str(home)}|(extra or {}),input=data,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=70)
  row=dict(arguments=list(map(str,args)),code=r.returncode,stdout=r.stdout.decode(errors='replace'),stderr=r.stderr.decode(errors='replace'),seconds=time.monotonic()-at)
  with log_lock:
   rows.append(row);(out/'operations.json').write_text(json.dumps(rows,indent=2)+'\n')
  assert (r.returncode==0)==ok,row
  return r
 def config(home,name):return json.loads(operation(home,'-e',name,'config','show','--json').stdout)
 def directory(home,name):return Path(operation(home,'env','path',name).stdout.decode().strip())
 def socket_path(home,name):return Path('/tmp')/f'trames-{os.getuid()}'/(hashlib.sha256(str(directory(home,name)).encode()).hexdigest()[:32]+'.console')
 def cold(home,source):
  operation(home,'config','--global','set','defaults.components.vulkan','false','defaults.display.windows','false','defaults.resources.cpus','1')
  operation(home,'config','--global','set','defaults.image.distribution','debian')
  operation(home,'config','--global','set','defaults.image.distribution','alpine')
  operation(home,'env','create','work','--rootfs',source)
  first=config(home,'work');work=directory(home,'work');assert len(first['environment']['uuid'])==36
  operation(home,'env','create','work','--rootfs',source,ok=False)
  old=(work/'env.toml').read_bytes() if (work/'env.toml').exists() else None
  operation(home,'-e','work','config','set','resources.memory','64MiB','resources.cpus','0',ok=False)
  assert ((work/'env.toml').read_bytes() if (work/'env.toml').exists() else None)==old
  operation(home,'-e','work','config','set','image','{}',ok=False)
  operation(home,'-e','work','config','set','resources.typo','5',ok=False)
  operation(home,'-e','work','config','set','resources.memory','--help',ok=False)
  operation(home,'-e','work','config','set','resources.memory','--global',ok=False)
  operation(home,'-e','work','config','set','resources.memory','64MiB','resources.cpus','1')
  operation(home,'-e','work','config','validate')
  operation(home,'config','--global','set','default_environment','work')
  assert b'64MiB' in operation(home,'config','get','resources.memory').stdout
  operation(home,'-e','work','config','edit',extra={'EDITOR':'/bin/false'},ok=False)
  busy=(home/'locks'/first['environment']['uuid']).open('a+b');fcntl.flock(busy,fcntl.LOCK_EX)
  try:
   operation(home,'env','remove','work','--yes',ok=False)
   operation(home,'env','backup','work',home/'busy.tar.gz',ok=False)
   operation(home,'env','clone','work','busy-clone',ok=False)
  finally:busy.close()
  shared=home/'shared';shared.mkdir();operation(home,'env','share','work','project',shared,'/work','ro')
  operation(home,'env','clone','work','copy')
  copy=config(home,'copy');assert copy['environment']['uuid']!=first['environment']['uuid'] and copy['mounts']==[]
  assert 'project' not in (directory(home,'copy')/'grants.toml').read_text()
  archive=home/'backup.tar.gz';operation(home,'env','backup','work',archive)
  operation(home,'env','backup','work',archive,ok=False)
  operation(home,'env','restore','restored',archive)
  restored=config(home,'restored');assert restored['mounts']==[] and restored['environment']['uuid']!=first['environment']['uuid']
  damaged=home/'corrupt.tar.gz';b=bytearray(archive.read_bytes());b[-5]^=255;damaged.write_bytes(b)
  operation(home,'env','restore','bad-crc',damaged,ok=False)
  traversal=home/'traversal.tar.gz'
  with tarfile.open(traversal,'w:gz') as t:
   h=tarfile.TarInfo('../escaped');h.size=1;t.addfile(h,io.BytesIO(b'x'))
  operation(home,'env','restore','bad-path',traversal,ok=False);assert not (home/'environments/escaped').exists()
  operation(home,'env','import-store','imported',work/'store')
  operation(home,'env','rename',first['environment']['uuid'],'renamed')
  assert config(home,'renamed')['environment']['uuid']==first['environment']['uuid']
  assert operation(home,'config','--global','get','default_environment').stdout.strip()==b'renamed'
  operation(home,'env','remove','copy',ok=False);operation(home,'env','remove','copy','--yes')
  assert shared.is_dir()
  p=directory(home,'restored')/'env.toml';p.write_text(p.read_text().replace('version = 1','version = 999',1))
  assert any(r['state']=='needs attention' for r in json.loads(operation(home,'env','ls','--json').stdout))
  operation(home,'-e','restored','config','validate',ok=False)
  assert not list((home/'environments').glob('.staging-*'))
  print('cold lifecycle: passed',flush=True)
 if a.native_only:
  source=out/'cold-root';(source/'bin').mkdir(parents=True);shutil.copy2(a.guest_cli or ROOT/'runtime/target/guest-cli/trames',source/'bin/sh');(source/'bin/sh').chmod(0o755)
  with tempfile.TemporaryDirectory(prefix='registry-',dir=out) as home:cold(Path(home),source)
 else:
  source=out/'rootfs';source.mkdir();copied=set()
  for path in ['/bin/sh','/bin/sleep','/bin/cat','/bin/echo','/usr/bin/env','/usr/bin/printf','/usr/bin/setsid','/usr/bin/uname','/usr/bin/sync','/bin/mkdir','/bin/rm','/bin/ls']:
   d.install(path,path,source,copied)
  for path in ['tmp','root','run','proc','dev','etc']:(source/path).mkdir(exist_ok=True)
  (out/'rootfs-inputs.json').write_text(json.dumps({str(p.relative_to(source)):d.digest(p) for p in source.rglob('*') if p.is_file()},indent=2,sort_keys=True)+'\n')
  for transport in ['smccc','mmio']:
   for cpus in [1,4]:
    label=f'{transport}-{cpus}';env['TRAMES_KVM_TRANSPORT']=transport
    with tempfile.TemporaryDirectory(prefix=label+'-',dir=out) as temp:
     home=Path(temp);live=[]
     try:
      cold(home,source)
      operation(home,'-e','renamed','config','set','resources.cpus',cpus,'resources.memory','auto')
      operation(home,'env','start','renamed');live.append('renamed')
      payload=b'a\0b\xff\n'
      result=operation(home,'-e','renamed','exec','--','/bin/cat',data=payload);assert result.stdout==payload
      result=operation(home,'-e','renamed','exec','--setenv','KEY=literal $() ; value','--cwd','/tmp','--','/bin/sh','-c','printf "%s|%s" "$KEY" "$PWD"; printf err >&2; exit 7',ok=False);assert result.returncode==7 and result.stdout==b'literal $() ; value|/tmp' and result.stderr==b'err'
      result=operation(home,'-e','renamed','exec','/bin/echo','--rootfs');assert result.stdout.strip()==b'--rootfs'
      operation(home,'-e','renamed','exec','--','/bin/sh','-c','echo durable > /root/value; sync')
      with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
       requests=[pool.submit(operation,home,'-e','renamed','exec','--','/bin/sh','-c',f'sleep .05; printf job{i}') for i in range(3)]
       assert [r.result().stdout for r in requests]==[b'job0',b'job1',b'job2']
      operation(home,'env','start','imported');live.append('imported')
      assert b'durable' not in operation(home,'-e','imported','exec','--','/bin/ls','/root').stdout
      operation(home,'env','remove','renamed','--yes',ok=False)
      operation(home,'env','backup','renamed',home/'live.tar.gz',ok=False)
      operation(home,'env','stop','renamed');live.remove('renamed')
      assert b'durable' in operation(home,'-e','renamed','exec','--','/bin/cat','/root/value').stdout;live.append('renamed')
      # Kernel peer credentials identify only this test's actual owner process.
      with socket.socket(socket.AF_UNIX) as peer:
       peer.connect(str(socket_path(home,'renamed').with_suffix('.exec')));pid,uid,gid=struct.unpack('3i',peer.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12));assert uid==os.getuid()
      os.kill(pid,signal.SIGKILL)
      end=time.monotonic()+10
      while True:
       state=json.loads(operation(home,'env','inspect','renamed','--json').stdout)
       if state['state']=='stopped':break
       assert time.monotonic()<end;time.sleep(.02)
      assert b'durable' in operation(home,'-e','renamed','exec','--','/bin/cat','/root/value').stdout
      # Detach one real terminal, then attach to the same independent session.
      master,slave=pty.openpty();p=subprocess.Popen([str(runner),'-e','renamed','shell'],env=env|{'TRAMES_HOME':str(home)},stdin=slave,stdout=slave,stderr=slave,start_new_session=True);os.close(slave)
      data=b'';deadline=time.monotonic()+15
      while b'Attached terminal ' not in data:
       assert time.monotonic()<deadline
       if select.select([master],[],[],.1)[0]:data+=os.read(master,4096)
      os.write(master,(r"printf '\nTERMINAL_RESULT\n'"+"\n").encode())
      deadline=time.monotonic()+10
      while b'\r\nTERMINAL_RESULT\r\n' not in data:
       assert time.monotonic()<deadline
       if select.select([master],[],[],.1)[0]:data+=os.read(master,4096)
      os.write(master,b'\x1d');p.wait(timeout=10);assert p.returncode==0;os.close(master)
      sessions=json.loads(operation(home,'-e','renamed','session','ls','--json').stdout);assert sessions
      sid=sessions[0]['id'];master,slave=pty.openpty()
      p=subprocess.Popen([str(runner),'-e','renamed','session','attach',str(sid)],env=env|{'TRAMES_HOME':str(home)},stdin=slave,stdout=slave,stderr=slave,start_new_session=True);os.close(slave)
      attached=b'';deadline=time.monotonic()+10
      while b'Attached terminal ' not in attached:
       assert time.monotonic()<deadline
       if select.select([master],[],[],.1)[0]:attached+=os.read(master,4096)
      os.write(master,(r"printf '\nREATTACHED_RESULT\n'"+"\n").encode())
      deadline=time.monotonic()+10
      while b'\r\nREATTACHED_RESULT\r\n' not in attached:
       assert time.monotonic()<deadline
       if select.select([master],[],[],.1)[0]:attached+=os.read(master,4096)
      os.write(master,b'\x1d');p.wait(timeout=10);assert p.returncode==0;os.close(master)
      assert any(s['id']==sid for s in json.loads(operation(home,'-e','renamed','session','ls','--json').stdout))
      (out/(label+'-terminal.log')).write_bytes(data+attached)
      operation(home,'env','restart','renamed')
      for n in live:operation(home,'env','stop',n)
      live=[]
      if a.systemd_rootfs:
       operation(home,'env','create','services','--rootfs',a.systemd_rootfs,'--init','systemd','--cpus',cpus,'--memory','auto')
       operation(home,'env','start','services');live.append('services')
       # Real service readiness and an ExecStop marker distinguish PID1 shutdown
       # from host cancellation, without modifying the original source rootfs.
       script="""for n in $(seq 1 100); do test -S /run/systemd/private && break; sleep .1; done
cat > /run/systemd/system/trames-environment.service <<'UNIT'
[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/sh -c 'echo started > /root/service-started'
ExecStop=/bin/sh -c 'echo graceful > /root/service-stopped'
UNIT
systemctl daemon-reload
systemctl start trames-environment.service
systemctl is-active trames-environment.service
"""
       assert b'active' in operation(home,'-e','services','exec','--','/bin/sh','-ec',script).stdout
       operation(home,'env','stop','services');live.remove('services')
       last=json.loads(operation(home,'env','inspect','services','--json').stdout)['last_exit'];assert last['code']==0 and last['error'] is None,last
       operation(home,'-e','services','config','set','boot.mode','shell')
       assert operation(home,'-e','services','exec','--','/bin/cat','/root/service-stopped').stdout.strip()==b'graceful';live.append('services')
       operation(home,'env','stop','services');live.remove('services')
      print(label+': passed',flush=True)
     finally:
      logs=out/(label+'-logs');logs.mkdir()
      if (home/'logs').exists():
       for log in (home/'logs').iterdir():shutil.copy2(log,logs/log.name)
      for n in live:subprocess.run([str(runner),'env','stop',n],env=env|{'TRAMES_HOME':str(home)},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=65)
 (out/'results.json').write_text(json.dumps(dict(passed=True,complete=True,native_only=a.native_only,operations=len(rows),runner_sha256=d.digest(runner),rows=rows),indent=2)+'\n');print(json.dumps(dict(passed=True,operations=len(rows))))
if __name__=='__main__':
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--runner',type=Path,required=True);p.add_argument('--guest-cli',type=Path);p.add_argument('--receipt',type=Path);p.add_argument('--systemd-rootfs',type=Path);p.add_argument('--output',type=Path,required=True);p.add_argument('--native-only',action='store_true');main(p.parse_args())
