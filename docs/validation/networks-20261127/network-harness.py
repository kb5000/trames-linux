#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Disposable named-network acceptance with original sockets and real host peers."""
import argparse,concurrent.futures,hashlib,importlib.util,json,os,select,shutil,socket,struct,subprocess,tempfile,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('desktop',ROOT/'scripts/validate-desktop.py');d=importlib.util.module_from_spec(spec);spec.loader.exec_module(d)
def main(a):
 out=a.output.resolve();out.mkdir(parents=True,exist_ok=False);tools=out/'tools';tools.mkdir();runner=tools/'runner';shutil.copy2(a.runner,runner);runner.chmod(0o555);rows=[]
 shutil.copy2(__file__,out/'harness.py')
 env=dict(os.environ)
 for key in ['DISPLAY','WAYLAND_DISPLAY','TRAMES_HOME']:env.pop(key,None)
 def operation(home,*args,ok=True,data=None):
  began=time.monotonic();r=subprocess.run([str(runner),*map(str,args)],env=env|{'TRAMES_HOME':str(home)},input=data,capture_output=True,timeout=60)
  row=dict(arguments=list(map(str,args)),code=r.returncode,stdout=r.stdout.decode(errors='replace'),stderr=r.stderr.decode(errors='replace'),seconds=time.monotonic()-began)
  rows.append(row);(out/'operations.json').write_text(json.dumps(rows,indent=2)+'\n');assert (r.returncode==0)==ok,row;return r
 def config(home,n):return json.loads(operation(home,'-e',n,'config','show','--json').stdout)
 def execute(home,n,*args,**kw):return operation(home,'-e',n,'exec','--','/bin/netprobe',*args,**kw)
 def cold(home,source):
  operation(home,'config','--global','set','defaults.components.vulkan','false','defaults.display.windows','false','defaults.resources.cpus','1')
  for n in ['lan','other']:operation(home,'network','create',n,'--internal')
  operation(home,'network','create','lan',ok=False);operation(home,'network','create','../escape',ok=False)
  for n,network in [('a','lan'),('b','lan'),('c','other')]:
   operation(home,'env','create',n,'--rootfs',source);operation(home,'-e',n,'config','set','network.mode','bridge','network.bridge',network)
  operation(home,'env','publish','a','38080','41000');assert config(home,'a')['network']['publish'][0]['host']=='127.0.0.1:38080'
  operation(home,'env','publish','a','38080','41001');assert len(config(home,'a')['network']['publish'])==1
  operation(home,'env','unpublish','a','38080');assert config(home,'a')['network']['publish']==[]
  before=config(home,'a');operation(home,'env','publish','a','0','41000',ok=False);operation(home,'env','publish','a','38080','0',ok=False);assert config(home,'a')==before
  operation(home,'env','grant','a','ports','off');operation(home,'env','publish','a','38080','41000',ok=False)
  operation(home,'env','grant','a','ports','on');operation(home,'-e','a','config','set','interop.ports.enabled','false');operation(home,'env','publish','a','38080','41000',ok=False);operation(home,'-e','a','config','set','interop.ports.enabled','true')
  return 0
 if a.native_only:
  source=out/'rootfs';(source/'bin').mkdir(parents=True);shutil.copy2(a.guest_cli or ROOT/'runtime/target/guest-cli/trames',source/'bin/sh');(source/'bin/sh').chmod(0o755)
  with tempfile.TemporaryDirectory(prefix='registry-',dir=out) as home:
   cold(Path(home),source);operation(home,'network','remove','other');operation(home,'network','remove','lan')
 else:
  assert a.receipt,'KVM needs --receipt';receipt=json.loads(a.receipt.read_text());assert receipt['fingerprint']['binary_sha256']==d.digest(runner);shutil.copy2(a.receipt,out/'build-receipt.json')
  (out/'doctor.json').write_bytes(subprocess.check_output([str(runner),'doctor','--json','--require-kvm']))
  probe=tools/'network-service';argv=['cc','-O2','-Wall','-Wextra','-Werror','-static',str(ROOT/'tests/fixtures/network-service.c'),'-o',str(probe)];subprocess.run(argv,check=True);shutil.copy2(ROOT/'tests/fixtures/network-service.c',out/'fixture.c')
  source=out/'rootfs';source.mkdir();copied=set()
  for f in ['/bin/sh','/usr/bin/setsid']:d.install(f,f,source,copied)
  shutil.copy2(probe,source/'bin/netprobe')
  for path in ['tmp','run','root','etc','dev','proc']:(source/path).mkdir(exist_ok=True)
  (out/'rootfs-inputs.json').write_text(json.dumps({str(p.relative_to(source)):d.digest(p) for p in source.rglob('*') if p.is_file()},indent=2)+'\n')
  def free_port(kind=socket.SOCK_STREAM,family=socket.AF_INET):
   with socket.socket(family,kind) as s:s.bind(('::1' if family==socket.AF_INET6 else '127.0.0.1',0));return s.getsockname()[1]
  def host_echo(ip,port,payload,tcp=True):
   family=socket.AF_INET6 if ':' in ip else socket.AF_INET
   with socket.socket(family,socket.SOCK_STREAM if tcp else socket.SOCK_DGRAM) as c:
    c.settimeout(10);c.connect((ip,port))
    if tcp:
     def send():c.sendall(payload);c.shutdown(socket.SHUT_WR)
     with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
      pending=pool.submit(send);data=b''
      while True:
       b=c.recv(65536)
       if not b:break
       data+=b
      pending.result();assert data==payload,(len(data),len(payload))
    else:c.send(payload);assert c.recv(65536)==payload
  # Calibrate the unmodified native fixture before involving any KVM session.
  native=[]
  for ip in ['127.0.0.1','::1']:
   for protocol in ['tcp','udp']:
    hp=free_port(socket.SOCK_STREAM if protocol=='tcp' else socket.SOCK_DGRAM,socket.AF_INET6 if ':' in ip else socket.AF_INET)
    p=subprocess.Popen([str(probe),'server',protocol,ip,str(hp)],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    try:
     assert select.select([p.stdout],[],[],10)[0] and p.stdout.readline()==b'READY\n'
     host_echo(ip,hp,b'native-original',protocol=='tcp');p.wait(timeout=5);assert p.returncode==0
     native.append(dict(ip=ip,protocol=protocol,passed=True))
    finally:
     if p.poll() is None:p.kill();p.wait()
  (out/'native-reference.json').write_text(json.dumps(native,indent=2)+'\n')
  for transport in ['smccc','mmio']:
   for cpus in [1,4]:
    label=f'{transport}-{cpus}'
    if a.case and label not in a.case:continue
    case=out/label;case.mkdir();env['TRAMES_KVM_TRANSPORT']=transport;children=[];live=[]
    with tempfile.TemporaryDirectory(prefix='registry-',dir=case) as temp:
     home=Path(temp)
     def start(n):operation(home,'env','start',n);live.append(n) if n not in live else None
     def stop(n):operation(home,'env','stop',n);live.remove(n) if n in live else None
     def server(n,protocol='tcp',port=41000,count=1,ip=None):
      name=f'{n}-{len(children)}';err=(case/(name+'.stderr')).open('wb');p=subprocess.Popen([str(runner),'-e',n,'exec','--','/bin/netprobe','server',protocol,ip or '0.0.0.0',str(port),str(count)],env=env|{'TRAMES_HOME':str(home)},stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=err);err.close();children.append(p)
      assert select.select([p.stdout],[],[],15)[0],name
      line=p.stdout.readline();assert line==b'READY\n',(name,line,(case/(name+'.stderr')).read_bytes());return p
     def done(p):p.wait(timeout=15);assert p.returncode==0,(p.returncode,p.stdout.read())
     def available(port,udp=False):
      with socket.socket(socket.AF_INET,socket.SOCK_DGRAM if udp else socket.SOCK_STREAM) as s:
       if not udp:s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
       s.bind(('127.0.0.1',port))
     try:
      cold(home,source)
      for n in ['a','b','c']:operation(home,'-e',n,'config','set','resources.cpus',cpus);start(n)
      nets=json.loads(operation(home,'network','ls','--json').stdout);lan=next(n for n in nets if n['name']=='lan');ua=config(home,'a')['environment']['uuid'];ub=config(home,'b')['environment']['uuid'];aid=lan['members'][ua];bid=lan['members'][ub];bip=f'10.0.0.{bid}'
      p=server('b',count=2);payload=b'cross-process\0\xff';assert execute(home,'a','client','tcp',bip,'41000',data=payload).stdout==payload
      execute(home,'c','client','tcp',bip,'41000',data=b'isolated',ok=False);assert p.poll() is None
      assert execute(home,'a','client','tcp',bip,'41000',data=b'peer-still-alive').stdout==b'peer-still-alive';done(p)
      for ip in [bip,f'fd00::{bid:x}']:
       p=server('b','udp',count=2,ip='::' if ':' in ip else '0.0.0.0')
       for payload in [b'',bytes(range(251))*200]:assert execute(home,'a','client','udp',ip,'41000',data=payload).stdout==payload
       done(p)
      p=server('b',ip='::');assert execute(home,'a','client','tcp',f'fd00::{bid:x}','41000',data=b'ipv6-peer').stdout==b'ipv6-peer';done(p)
      p=server('a',ip=f'10.0.0.{aid}');q=server('b',ip=bip);assert execute(home,'a','client','tcp','127.0.0.1','41000',data=b'private-loopback',ok=False).returncode!=0
      # Listeners on eth0 in separate environments may use the same port.
      assert execute(home,'b','client','tcp',f'10.0.0.{aid}','41000',data=b'same-port-a').stdout==b'same-port-a';done(p)
      assert execute(home,'a','client','tcp',bip,'41000',data=b'same-port-b').stdout==b'same-port-b';done(q)
      p=server('a',ip='127.0.0.1');q=server('b',ip='127.0.0.1')
      execute(home,'a','client','tcp',bip,'41000',data=b'not-peer-loopback',ok=False);assert q.poll() is None
      assert execute(home,'b','client','tcp','127.0.0.1','41000',data=b'private-loop-b').stdout==b'private-loop-b';done(q)
      assert execute(home,'a','client','tcp','127.0.0.1','41000',data=b'private-loop-a').stdout==b'private-loop-a';done(p)
      assert execute(home,'a','resolve',f'env{aid}.trames').stdout.strip()==f'10.0.0.{aid}'.encode()
      # One reachable native listener is both the negative and positive control.
      with socket.socket(socket.AF_INET,socket.SOCK_DGRAM) as route:route.connect(('192.0.2.1',9));host_ip=route.getsockname()[0]
      assert not host_ip.startswith('127.')
      with socket.socket() as native:
       native.bind((host_ip,0));native.listen(4);native_port=native.getsockname()[1]
       execute(home,'c','client','tcp',host_ip,native_port,data=b'no-nat',ok=False)
       native.setblocking(False)
       try:
        leaked,_=native.accept();leaked.close();raise AssertionError('internal network reached the native host listener')
       except BlockingIOError:pass
       native.settimeout(10)
       operation(home,'network','create','uplink');operation(home,'env','create','nat','--rootfs',source,'--cpus',cpus)
       operation(home,'-e','nat','config','set','network.mode','bridge','network.bridge','uplink');start('nat')
       def reply():
        peer,_=native.accept()
        with peer:
         while True:
          b=peer.recv(65536)
          if not b:break
          peer.sendall(b)
       with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
        future=pool.submit(reply);assert execute(home,'nat','client','tcp',host_ip,native_port,data=b'controlled-nat').stdout==b'controlled-nat';future.result(timeout=10)
      operation(home,'network','remove','lan',ok=False)
      port=free_port();operation(home,'env','publish','a',port,'41000');operation(home,'env','publish','a',port,'41000','udp');operation(home,'env','restart','a')
      assert next(n for n in json.loads(operation(home,'network','ls','--json').stdout) if n['name']=='lan')['members'][ua]==aid
      p=server('a');host_echo('127.0.0.1',port,bytes(range(256))*8192);done(p)
      p=server('a','udp',count=3)
      for payload in [b'udp-a',b'',bytes(range(251))*200]:host_echo('127.0.0.1',port,payload,False)
      done(p)
      stop('c');partial=free_port(socket.SOCK_DGRAM);operation(home,'env','publish','c',partial,'41001','udp');operation(home,'env','publish','c',port,'41000');operation(home,'env','start','c',ok=False);available(partial,True)
      stop('a');available(port);available(port,True);operation(home,'env','start','c');live.append('c');stop('c')
      operation(home,'env','grant','a','ports','off');operation(home,'env','start','a',ok=False);operation(home,'env','grant','a','ports','on');start('a')
      p=server('a');host_echo('127.0.0.1',port,b'restarted-port');done(p)
      # Explicit wildcard and IPv6 publication use the actual host addresses.
      for ip,host,guest in [('127.0.0.1','0.0.0.0',41002),('::1','::1',41003)]:
       hp=free_port(family=socket.AF_INET6 if ':' in ip else socket.AF_INET);address=f'[{host}]:{hp}' if ':' in host else f'{host}:{hp}'
       operation(home,'env','publish','a',address,guest);operation(home,'env','publish','a',address,guest,'udp');operation(home,'env','restart','a')
       p=server('a',port=guest,ip='::' if ':' in ip else '0.0.0.0');host_echo(ip,hp,b'host-address');done(p)
       p=server('a','udp',port=guest,ip='::' if ':' in ip else '0.0.0.0');host_echo(ip,hp,b'host-udp',False);done(p)
      for n in ['d','e']:operation(home,'env','create',n,'--rootfs',source,'--cpus',cpus);start(n)
      hp=free_port();p=server('d',port=hp,ip='127.0.0.1');execute(home,'e','server','tcp','127.0.0.1',hp,ok=False);host_echo('127.0.0.1',hp,b'native-host-space');done(p)
      # Observe this test's actual worker through kernel peer credentials.
      directory=Path(operation(home,'env','path','a').stdout.decode().strip())
      endpoint=Path('/tmp')/f'trames-{os.getuid()}'/(hashlib.sha256(str(directory).encode()).hexdigest()[:32]+'.exec')
      with socket.socket(socket.AF_UNIX) as peer:peer.connect(str(endpoint));pid,uid,gid=struct.unpack('3i',peer.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12));assert uid==os.getuid()
      def ticks():
       fields=Path(f'/proc/{pid}/stat').read_text().rsplit(')',1)[1].split();return int(fields[11])+int(fields[12])
      before=ticks();time.sleep(1);cpu=(ticks()-before)/os.sysconf('SC_CLK_TCK');assert cpu<.25,dict(id=label,idle_cpu_seconds=cpu)
      (case/'idle.json').write_text(json.dumps(dict(seconds=1,cpu_seconds=cpu,qualified_performance=False))+'\n')
      for n in live[:]:stop(n)
      operation(home,'network','remove','lan');operation(home,'network','remove','other');operation(home,'network','remove','uplink')
      print(label+': passed',flush=True)
     finally:
      logs=case/'logs';logs.mkdir()
      if (home/'logs').exists():
       for log in (home/'logs').iterdir():shutil.copy2(log,logs/log.name)
      for p in children:
       if p.poll() is None:p.terminate()
      for n in live:subprocess.run([str(runner),'env','stop',n],env=env|{'TRAMES_HOME':str(home)},capture_output=True,timeout=60)
      for p in children:
       if p.poll() is None:
        try:p.wait(timeout=5)
        except subprocess.TimeoutExpired:p.kill();p.wait()
 (out/'results.json').write_text(json.dumps(dict(passed=True,complete=True,native_only=a.native_only,runner_sha256=d.digest(runner),operations=len(rows),rows=rows),indent=2)+'\n');print(json.dumps(dict(passed=True,operations=len(rows))))
if __name__=='__main__':
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--runner',type=Path,required=True);p.add_argument('--receipt',type=Path);p.add_argument('--guest-cli',type=Path);p.add_argument('--output',type=Path,required=True);p.add_argument('--native-only',action='store_true');p.add_argument('--case',action='append');main(p.parse_args())
