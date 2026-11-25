from pathlib import Path
import csv,hashlib,json,os,subprocess,sys
root=Path('/home/main.guest/trames-linux-wsi-qualified')
sys.path.insert(0,str(root/'runtime/tests/acceptance'))
from common import save,digest
from run import execute,receipt,host
runner=Path('/home/main.guest/trames-memory-binary-v9')
build=receipt(Path('/home/main.guest/trames-memory-build-v9.json'),runner)

def root_exists(path):
 return subprocess.run(['sudo','-n','test','-e',str(path)],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0

diag=Path('/home/main.guest/trames-performance-perf-v10');diag.mkdir(exist_ok=False)
environment=dict(PATH='/usr/bin:/bin',HOME='/tmp',LANG='C',LC_ALL='C',TZ='UTC')
trace=Path('/sys/kernel/tracing')
if not root_exists(trace/'events/kvm/kvm_exit'):
 p=subprocess.run(['sudo','-n','mount','-t','tracefs','tracefs',str(trace)],capture_output=True)
 (diag/'tracefs.stdout').write_bytes(p.stdout);(diag/'tracefs.stderr').write_bytes(p.stderr)
requested=['kvm:kvm_exit','syscalls:sys_enter_ioctl','sched:sched_switch',
 'syscalls:sys_enter_timer_create','syscalls:sys_enter_timer_settime',
 'syscalls:sys_enter_timer_delete','syscalls:sys_enter_rt_sigprocmask',
 'syscalls:sys_enter_rt_sigtimedwait','syscalls:sys_enter_gettid']
events=[event for event in requested if root_exists(trace/'events'/event.replace(':','/'))]
save(diag/'inputs.json',dict(build=build,host=host(),events=events,unavailable_events=sorted(set(requested)-set(events)),instrumented=True,primary_baseline=False,
                          perf_version=subprocess.check_output(['perf','--version'],text=True),controller_sha256=digest(Path(__file__))))
if not root_exists(trace/'events/kvm/kvm_exit'):
 save(diag/'results.json',dict(status='unavailable',reason='KVM tracepoints unavailable; no counter claim'))
 raise SystemExit(0)
base=Path('/home/main.guest/trames-performance-baseline-v9/rootfs')
work=base/'work/diagnostic';work.mkdir(exist_ok=False)
records=[]
calibrations={(row['mode'],row['workers']):{k:v for k,v in row['work'].items() if k!='work_ns'}
 for row in json.loads((base.parent/'samples.json').read_text()) if row['phase']=='calibration'}
for mode,count in [('cpu',50000000),('getpid',10000)]:
 for lane in ['native','smccc','mmio']:
  folder=diag/(mode+'-'+lane);folder.mkdir()
  env=environment|({'TRAMES_KVM_TRANSPORT':lane} if lane!='native' else {})
  if lane=='native': command=[str(base/'test/performance'),mode,str(count),'1',str(work)]
  else: command=[str(runner),'run','--cpus','1','--rootfs',str(base),'--timeout-ms','30000','--','/test/performance',mode,str(count),'1','/work/diagnostic']
  command=['sudo','-n','env',*[k+'='+v for k,v in env.items()],'perf','stat','--no-big-num','-x,','-o',str(folder/'counters.csv'),'-e',','.join(events),'--','taskset','-c','2',*command]
  save(folder/'command.json',dict(argv=command))
  result=execute(command,os.environ.copy(),folder,35)
  counters={}
  if (folder/'counters.csv').exists():
   for row in csv.reader((folder/'counters.csv').read_text().splitlines()):
    if len(row)>2 and row[2] in events:
     try:counters[row[2]]=float(row[0])
     except ValueError:counters[row[2]]=None
  try: semantic={k:v for k,v in json.loads((folder/'stdout').read_text()).items() if k!='work_ns'}
  except (ValueError,AttributeError):semantic=None
  passed=result['status']=='completed' and result['exit_code']==0 and semantic==calibrations[(mode,1)] and set(counters)==set(events) and all(v is not None for v in counters.values())
  records.append(dict(id=folder.name,passed=passed,**result,counters=counters,logs={s:digest(folder/s) for s in ['stdout','stderr','command.json','counters.csv'] if (folder/s).exists()}))
  print('instrumented diagnostic',records[-1],flush=True)
save(diag/'results.json',dict(status='passed' if all(r['passed'] for r in records) else 'partial',instrumented=True,primary_baseline=False,cases=records))
work.rmdir()
