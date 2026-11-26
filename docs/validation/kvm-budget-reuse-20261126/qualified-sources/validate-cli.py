#!/usr/bin/env python3
"""Shared CLI frontend against original ARM64 binaries and a real host PTY."""
from pathlib import Path
import fcntl,json,os,pty,select,signal,struct,subprocess,termios,time
root=Path(__file__).resolve().parents[1]
os.chdir(root)
binary=Path(os.environ.get('TRAMES_BINARY','target/release/trames')).resolve()
out=Path(os.environ.get('TRAMES_VALIDATION_OUTPUT','target/validation/cli')).resolve();out.mkdir(parents=True,exist_ok=True)
fixture=out/'cli-static'
subprocess.run(['cc','-O2','-static','tests/fixtures/cli.c','-o',str(fixture)],check=True)
results=[]
def record(name,passed,**detail):
    results.append(dict(name=name,passed=passed,**detail));print(name,'PASS' if passed else 'FAIL',flush=True)
payload=bytes(range(256))*3000
native=subprocess.run([str(fixture),'stdio'],input=payload,capture_output=True,timeout=10)
guest=subprocess.run([str(binary),'run','--',str(fixture),'stdio'],input=payload,capture_output=True,timeout=10)
record('binary-stdio',native.returncode==guest.returncode==7 and native.stdout==guest.stdout==payload and native.stderr==guest.stderr==b'guest-stderr\n',code=guest.returncode,bytes=len(guest.stdout))
rootfs=out/'rootfs';(rootfs/'bin').mkdir(parents=True,exist_ok=True);(rootfs/'tmp').mkdir(exist_ok=True)
import shutil
shutil.copy2(fixture,rootfs/'bin/cli')
guest=subprocess.run([str(binary),'exec','--rootfs',str(rootfs),'--cwd','/tmp','--setenv','CLI_VALUE=a b;$(id)','--','/bin/cli','env'],capture_output=True,timeout=10)
record('literal-env-cwd',guest.returncode==0 and guest.stdout==b'/tmp:a b;$(id)\n',code=guest.returncode,stdout=guest.stdout.decode(errors='replace'),stderr=guest.stderr.decode(errors='replace'))
# Exercise the advertised shell entry with an original dynamic host shell.
shutil.copy2('/bin/sh',rootfs/'bin/sh')
for line in subprocess.check_output(['ldd','/bin/sh'],text=True).splitlines():
    for name in [word for word in line.split() if word.startswith('/')]:
        path=Path(name);dest=rootfs/path.relative_to('/');dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(path,dest)
shell=subprocess.run([str(binary),'shell','--rootfs',str(rootfs),'--no-pty','--timeout-ms','5000'],input=b"printf 'shell-ok\\n'\nexit 9\n",capture_output=True,timeout=10)
record('explicit-rootfs-shell',shell.returncode==9 and shell.stdout==b'shell-ok\n',code=shell.returncode,stdout=shell.stdout.decode(errors='replace'),stderr=shell.stderr.decode(errors='replace'))
for mode,sig in [('spin',signal.SIGINT),('syscall-spin',signal.SIGINT),('sleep',signal.SIGTERM)]:
    p=subprocess.Popen([str(binary),'run','--',str(fixture),mode],stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    time.sleep(.15);began=time.monotonic();p.send_signal(sig)
    try:stdout,stderr=p.communicate(timeout=3);record('cancel/'+mode,p.returncode==128+sig,code=p.returncode,seconds=time.monotonic()-began,stderr=stderr.decode(errors='replace'))
    except subprocess.TimeoutExpired:p.kill();p.communicate();record('cancel/'+mode,False,error='timeout')
# A stream of short SVCs must not renew its quantum forever or suppress host cancellation.
for mode in ['spin','syscall-spin','sleep']:
    began=time.monotonic()
    try:
        result=subprocess.run([str(binary),'run','--timeout-ms','200','--',str(fixture),mode],stdin=subprocess.DEVNULL,capture_output=True,timeout=4)
        elapsed=time.monotonic()-began
        record('deadline/'+mode,result.returncode==2 and result.stdout==b'' and result.stderr==b'trames: session cancelled by its host\n' and .15<=elapsed<3,code=result.returncode,seconds=elapsed,stderr=result.stderr.decode(errors='replace'))
    except subprocess.TimeoutExpired:
        record('deadline/'+mode,False,error='timeout')
master,slave=pty.openpty();fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',31,99,0,0));saved=termios.tcgetattr(slave)
p=subprocess.Popen([str(binary),'run','--pty','--',str(fixture),'tty'],stdin=slave,stdout=slave,stderr=slave,close_fds=True)
data=b'';sent=False;began=time.monotonic()
while time.monotonic()-began<10:
    if select.select([master],[],[],.1)[0]:
        try:chunk=os.read(master,8192)
        except OSError:break
        data+=chunk
        if b'tty:' in data and not sent:os.write(master,b'hello cli\n');sent=True
    if p.poll() is not None:
        while select.select([master],[],[],.05)[0]:
            try:data+=os.read(master,8192)
            except OSError:break
        break
if p.poll() is None:p.kill()
p.wait(timeout=3)
restored=termios.tcgetattr(slave)
# PENDIN is an implementation-maintained tty retype flag on macOS.
record('real-pty-size-and-restore',p.returncode==0 and b'tty:31,99' in data and b'line:hello cli' in data and (restored[3]&~getattr(termios,'PENDIN',0))==(saved[3]&~getattr(termios,'PENDIN',0)),code=p.returncode,output=data.decode(errors='replace'))
os.close(master);os.close(slave)
(out/'results.json').write_text(json.dumps(results,indent=2)+'\n')
if any(not x['passed'] for x in results):raise SystemExit('CLI validation failures')
