#!/usr/bin/env python3
"""Compare original native ARM64 ELF workloads with the shared KVM session."""
from pathlib import Path
import json,os,shutil,subprocess,time
root=Path(__file__).resolve().parents[1]
os.chdir(root)
binary=Path(os.environ.get('TRAMES_BINARY','target/release/trames')).resolve()
output=Path(os.environ.get('TRAMES_VALIDATION_OUTPUT','target/validation/session')).resolve()
output.mkdir(parents=True,exist_ok=True)
fixture=output/'session-static'
subprocess.run(['cc','-O2','-static','-pthread','tests/fixtures/session.c','-o',str(fixture)],check=True)
subprocess.run([str(binary),'doctor','--require-kvm','--json'],check=True,stdout=(output/'doctor.json').open('w'))
results=[]
def run(name,argv,expected,expected_stdout=None,limit=30):
    begin=time.monotonic()
    try:
        p=subprocess.run(argv,capture_output=True,timeout=limit)
        passed=p.returncode==expected and (expected_stdout is None or p.stdout==expected_stdout)
        result=dict(name=name,passed=passed,code=p.returncode,stdout=p.stdout.decode(errors='replace'),stderr=p.stderr.decode(errors='replace'),seconds=time.monotonic()-begin)
    except subprocess.TimeoutExpired:
        result=dict(name=name,passed=False,error='timeout',seconds=time.monotonic()-begin)
    results.append(result)
    print(name,'PASS' if result['passed'] else 'FAIL',flush=True)
    return result
for mode in ('basic','memory','fork','threads','exit'):
    code=37 if mode=='exit' else 0
    native=run('native/'+mode,[str(fixture),mode],code)
    for cpus in (1,4):
        run(f'kvm-{cpus}/'+mode,[str(binary),'run','--cpus',str(cpus),'--timeout-ms','20000','--',str(fixture),mode],code,native.get('stdout','').encode())
# A deadline must wake both a CPU loop and an otherwise sleeping session.
for mode in ('spin','sleep'):
    begin=time.monotonic()
    try:
        p=subprocess.run([str(binary),'run','--timeout-ms','100','--',str(fixture),mode],capture_output=True,timeout=4)
        elapsed=time.monotonic()-begin
        result=dict(name='deadline/'+mode,passed=p.returncode!=0 and elapsed<3,code=p.returncode,seconds=elapsed,stderr=p.stderr.decode(errors='replace'))
    except subprocess.TimeoutExpired:
        result=dict(name='deadline/'+mode,passed=False,error='timeout')
    results.append(result);print(result['name'],'PASS' if result['passed'] else 'FAIL',flush=True)
# A real dynamic loader and libc are copied into an isolated test rootfs.
dynamic=output/'session-dynamic'
subprocess.run(['cc','-O2','-pthread','tests/fixtures/session.c','-o',str(dynamic)],check=True)
rootfs=output/'rootfs'
for name in ('bin','dev','proc','tmp'):(rootfs/name).mkdir(parents=True,exist_ok=True)
shutil.copy2(dynamic,rootfs/'bin/session')
ldd=subprocess.check_output(['ldd',str(dynamic)],text=True)
for line in ldd.splitlines():
    paths=[word for word in line.split() if word.startswith('/')]
    for name in paths:
        path=Path(name);dest=rootfs/path.relative_to('/');dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(path,dest)
for mode in ('basic','memory','fork','threads'):
    run('dynamic-rootfs/'+mode,[str(binary),'run','--rootfs',str(rootfs),'--cpus','4','--timeout-ms','20000','--','/bin/session',mode],0,(mode+': ok\n').encode())
(output/'results.json').write_text(json.dumps(results,indent=2)+'\n')
failed=[p['name'] for p in results if not p['passed']]
if failed:raise SystemExit('session failures: '+', '.join(failed))
print('all native/KVM session comparisons passed')
