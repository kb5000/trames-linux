#!/usr/bin/env python3
"""Offline review of the joined-epoch KVM batch; no workload or VM execution."""
from pathlib import Path
import importlib.util,json,re,sys,tarfile,tempfile
HERE=Path(__file__).resolve().parent
PREVIOUS=HERE.parent/'kvm-timer-reuse-20261126'
sys.path.insert(0,str(PREVIOUS/'audit-support'))
from common import digest,identity
from run import verified_evidence
spec=importlib.util.spec_from_file_location('frozen_benchmark',PREVIOUS/'audit-controller.py')
benchmark=importlib.util.module_from_spec(spec);spec.loader.exec_module(benchmark)
def read(path):return json.loads(path.read_text())
index=read(HERE/'evidence-index.json')
for name,value in index['files'].items():assert digest(HERE/name)==value,name
checks={}
with tempfile.TemporaryDirectory(prefix='trames-joined-writes-audit-') as temporary:
 root=Path(temporary)
 with tarfile.open(HERE/'raw-evidence.tar.gz','r:gz') as archive:
  members=archive.getmembers()
  assert len(members)<15000 and sum(m.size for m in members)<48*1024*1024
  for member in members:
   p=Path(member.name);assert not p.is_absolute() and '..' not in p.parts and (member.isfile() or member.isdir())
  archive.extractall(root)
 build=read(root/'build.json');baseline=read(root/'baseline-build.json')
 for receipt in [build,baseline]:
  assert identity(receipt['fingerprint'])==receipt['fingerprint_sha256']
  assert receipt['sources']['runtime']['commit']=='a37a84d4b3c66f21ddb581f8f4b0427f50793e0d'
  assert all(not source['dirty'] for source in receipt['sources'].values())
 assert build['sources']['product']['commit'].startswith('c095588')
 assert baseline['sources']['product']['commit'].startswith('88b6d85')
 assert build['fingerprint']['toolchain']==baseline['fingerprint']['toolchain']
 assert build['fingerprint']['flags']==baseline['fingerprint']['flags']
 original=root/'original-performance';retry=root/'retry-performance'
 assert benchmark.audit(original)==1
 assert read(original/'results.json')['failures']==['build-1-sample-1-baseline-smccc','incomplete distribution: build/1/baseline-smccc']
 assert benchmark.audit(retry)==0 and len(read(retry/'samples.json'))==10
 selected=read(root/'performance/samples.json');origin_rows={label:{r['id']:r for r in read(folder/'samples.json')} for label,folder in [('original',original),('retry',retry)]}
 expected={('original',r['id']) for r in origin_rows['original'].values() if not (r['mode']=='build' and r['workers']==1 and r['phase']=='sample' and r['repeat']==1)}
 expected.update(('retry',r['id']) for r in origin_rows['retry'].values() if r['phase']=='sample')
 assert len(expected)==240
 def verify_selection(rows):
  assert {(r['origin']['dataset'],r['origin']['id']) for r in rows}==expected and len(rows)==240
  for row in rows:
   origin=row['origin'];label=origin['dataset'];identity=origin['id'];source=origin_rows[label][identity]
   assert row['id']==label+'/'+identity
   assert {k:v for k,v in row.items() if k not in ['id','origin']}=={k:v for k,v in source.items() if k!='id'}
 verify_selection(selected)
 checks['paired_round_retry_selection_verified']=True
 bad=json.loads(json.dumps(selected));bad[-1]['origin']={'dataset':'original','id':'build-1-sample-1-baseline-smccc'}
 try:verify_selection(bad)
 except AssertionError:pass
 else:raise AssertionError('partial or favorable round selection was accepted')
 bad=json.loads(json.dumps(selected));bad[-1]['wall_ns']+=1
 try:verify_selection(bad)
 except AssertionError:pass
 else:raise AssertionError('derived timing differs from original source')
 checks['selection_negative_cases']=2
 a=read(original/'inputs.json');b=read(retry/'inputs.json')
 for key in ['build','baseline_build','compiler_version','fixture_sources','affinity','controller_sha256']:assert a[key]==b[key],key
 assert a['compiler_command'][:-1]==b['compiler_command'][:-1] and a['compiler_command'][-2]=='-o'
 assert all(a['versions'][k]==v for k,v in b['versions'].items())
 assert all(a['immutable_files'][k]==v for k,v in b['immutable_files'].items())
 for name,folder in [('original',original),('retry',retry)]:
  origin=read(root/'performance/inputs.json')['derivation']['sources'][name]
  assert origin['inputs_sha256']==digest(folder/'inputs.json') and origin['samples_sha256']==digest(folder/'samples.json')
 row=origin_rows['original']['build-1-sample-1-baseline-smccc'];folder=original/'cases'/row['id']
 assert row['exit_code']==0 and not row['timed_out']
 assert 'Clock skew detected.' in (folder/'stderr').read_text()
 native=next(r for r in origin_rows['original'].values() if r['mode']=='build' and r['workers']==1 and r['phase']=='calibration')
 assert {k:v for k,v in row['work'].items() if k!='work_ns'}=={k:v for k,v in native['work'].items() if k!='work_ns'}
 checks['retained_original_clock_warning']=True;checks['supplemental_raw_executions']=10
 inputs=read(root/'performance/inputs.json')
 assert inputs['build']==build and inputs['baseline_build']==baseline
 assert digest(PREVIOUS/'audit-controller.py')==inputs['controller_sha256']
 assert inputs['host']['page_size']==4096 and inputs['repeats']==3
 assert set(inputs['workloads'])=={'cpu','memory','getpid','mmap','build','node'}
 assert inputs['workers']==[1,4] and set(inputs['lanes'])=={'native','baseline-smccc','candidate-smccc','baseline-mmio','candidate-mmio'}
 for name,value in inputs['fixture_sources'].items():assert digest(HERE/'qualified-sources'/Path(name).name)==value
 assert benchmark.audit(root/'performance')==0
 assert read(root/'performance/results.json')==read(HERE/'performance/results.json')
 rows=read(root/'performance/samples.json')
 assert len(rows)==240 and sum(r['phase']=='sample' for r in rows)==180
 checks['performance_total_executions']=240;checks['performance_measured_samples']=180
 initial=read(root/'initial-build.json');assert initial['sources']['product']['commit'].startswith('6259a61')
 assert identity(initial['fingerprint'])==initial['fingerprint_sha256']
 assert initial['sources']['runtime']==build['sources']['runtime']
 inputs=read(root/'initial-performance/inputs.json');assert inputs['build']==initial and inputs['baseline_build']==baseline
 assert benchmark.audit(root/'initial-performance')==0
 assert len(read(root/'initial-performance/samples.json'))==240
 assert read(root/'initial-performance/results.json')==read(HERE/'initial-performance/results.json')
 checks['retained_initial_trial_executions']=240

 previous=read(PREVIOUS/'audit-result.json')
 assert previous['passed'] and len(previous['negative_tests'])==4 and all(r['rejected'] for r in previous['negative_tests'])
 checks['unchanged_auditor_negative_cases_reused']=4
 modes=('geometry','protection','holes','owners','file-offsets','truncate','discard-remap','cow','threads','retire','exec')
 expected={f'native-{m}':m for m in modes};expected.update({f'{m}-{t}-{w}':m for m in modes for t in ['smccc','mmio'] for w in [1,2,4]})
 memory=read(root/'memory/results.json')
 assert memory['passed']==memory['planned']==len(memory['cases'])==77
 assert {r['id'] for r in memory['cases']}==set(expected)
 for row in memory['cases']:
  assert row['status']=='passed' and row['exit_code']==0
  folder=root/'memory'/row['id'];assert (folder/'stdout').read_bytes()==(expected[row['id']]+': ok\n').encode() and not (folder/'stderr').read_bytes()
 for domain,fixture,controller in [('memory','memory-4k.c','validate-memory.py'),('cpu','cpu-extensions.c','validate-cpu.py')]:
  inputs=read(root/domain/'inputs.json');assert inputs['build']==build and inputs['host']['page_size']==4096
  assert digest(HERE/'qualified-sources'/fixture)==inputs['fixture_source_sha256']
  assert digest(HERE/'qualified-sources'/controller)==inputs['controller_sha256']
 assert set(read(root/'memory/inputs.json')['load_alignments'])=={4096}
 checks['core_memory']=77
 cpu=read(root/'cpu/results.json');assert cpu['passed']==cpu['planned']==len(cpu['cases'])==15
 for row in cpu['cases']:
  assert row['status']=='passed' and row['exit_code']==0
  mode=row['id'].split('-')[1 if row['id'].startswith('native-') else 0]
  assert (root/'cpu'/row['id']/'stdout').read_bytes()==(root/'cpu'/('native-'+mode)/'stdout').read_bytes()
 checks['cpu_state_ptrace']=15
 for label in ['native','kvm-smccc-1','kvm-smccc-4','kvm-mmio-1','kvm-mmio-4']:
  folder=root/('applications-native' if label=='native' else 'applications/'+label)
  plan=read(folder/'plan.json');rows=read(folder/'results.json')['cases'];assert len(rows)==len(plan['cases'])==12
  by_id={r['id']:r for r in rows};assert set(by_id)=={c['id'] for c in plan['cases']}
  for case in plan['cases']:
   key=identity(dict(case=case,rootfs=plan['bundle_sha256'],harness=plan['harness_sha256']))
   fingerprint=identity(dict(inputs=key,provider=plan['provider'],host={k:v for k,v in plan['host'].items() if k!='cpu'}))
   assert verified_evidence(folder,by_id[case['id']],case,fingerprint),(label,case['id'])
 checks['application_kvm_cells']=48;checks['reused_native_application_cells']=12
 fuse=read(root/'fuse/results.json');assert fuse['planned']==fuse['passed']==len(fuse['cases'])==12 and fuse['checks_passed']==144
 assert read(root/'fuse/inputs.json')['build']==build
 assert {r['id'] for r in fuse['cases']}=={f'{p}-{t}-{w}' for p in ['lowlevel','writeback','highlevel'] for t in ['smccc','mmio'] for w in [1,4]}
 for row in fuse['cases']:
  folder=root/'fuse'/row['id'];assert row['passed'] and row['exit_code']==0 and all(row['checks'].values())
  for name,value in row['logs'].items():assert digest(folder/name)==value
  text=(folder/'stdout').read_text();assert all(text.count('CASE '+name+' PASS\n')==1 for name in row['checks'])
  assert (folder/'fuse-daemon.exit').read_text().strip()=='0'
  if row['id'].startswith('writeback-'):assert 'activating writeback' in (folder/'fuse-daemon.stderr').read_text()
 checks['stock_fuse_functional_and_shutdown']=144
 for transport in ['smccc','mmio']:
  text=(root/'logs'/f'trames-kvm-joined-writes-compact-hardware-{transport}.log').read_text()
  assert '10 passed; 0 failed' in text and '8 passed; 0 failed' in text
  assert 'retained_writes_republish_after_join_and_follow_original_address_space ... ok' in text
 checks['real_kvm_library_and_hardware']=36
 for transport in ['smccc','mmio']:
  text=(root/'logs'/f'trames-kvm-joined-writes-compact-vulkan-{transport}.log').read_text()
  assert '1 passed; 0 failed' in text and 'vulkan_host_buffer_and_kvm_share_original_bytes_without_shadow_transfers ... ok' in text
 checks['software_vulkan_and_kvm_4k_buffer_aliases']=2

 flat=(root/'profile-v3/flat.txt').read_text();values=re.findall(r'^\s*([0-9.]+)%\s+([0-9]+)\s+(.+)$',flat,re.M)
 assert 99<sum(float(r[0]) for r in values)<101 and sum(int(r[1]) for r in values)>1500
 assert '[g] 0xffffffffffff8b04' in flat and '[g] 0xffffffffffff8860' in flat
 checks['sampling_host_and_guest_accounted']=True
 result=dict(passed=True,checks=checks,raw_archive_sha256=digest(HERE/'raw-evidence.tar.gz'),full_product_4k_qualified=False)
 (HERE/'audit-result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
