#!/usr/bin/env python3
"""Audit retained evidence without KVM, rootfs images or benchmark reruns."""
from pathlib import Path
import hashlib,importlib.util,json,sys,tarfile,tempfile

HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE/'audit-support'))
from common import digest,identity
from run import verified_evidence
spec=importlib.util.spec_from_file_location('frozen_benchmark_auditor',HERE/'audit-controller.py')
benchmark=importlib.util.module_from_spec(spec);spec.loader.exec_module(benchmark)
index=json.loads((HERE/'evidence-index.json').read_text())
for name,expected in index['files'].items():assert digest(HERE/name)==expected,name
results={}
with tempfile.TemporaryDirectory(prefix='trames-memory-audit-') as temporary:
 root=Path(temporary)
 with tarfile.open(HERE/'raw-evidence.tar.gz','r:gz') as archive:
  members=archive.getmembers()
  assert len(members)<10000 and sum(m.size for m in members)<32*1024*1024
  for member in members:
   p=Path(member.name)
   assert not p.is_absolute() and '..' not in p.parts and (member.isfile() or member.isdir())
  archive.extractall(root)
 assert benchmark.audit(root/'performance')==0
 assert json.loads((root/'performance/results.json').read_text())==json.loads((HERE/'performance/results.json').read_text())
 results['performance_measured_samples']=585
 memory=json.loads((root/'memory/results.json').read_text())
 modes=('geometry','protection','holes','owners','file-offsets','truncate','discard-remap','cow','threads','retire','exec')
 expected={f'native-{mode}':mode for mode in modes}
 expected.update({f'{mode}-{transport}-{workers}':mode for mode in modes for transport in ['smccc','mmio'] for workers in [1,2,4]})
 assert memory['passed']==memory['planned']==len(memory['cases'])==77
 assert {r['id'] for r in memory['cases']}==set(expected)
 for row in memory['cases']:
  assert row['status']=='passed' and row['exit_code']==0
  folder=root/'memory'/row['id']
  assert (folder/'stdout').read_bytes()==(expected[row['id']]+': ok\n').encode() and not (folder/'stderr').read_bytes()
 results['core_memory']=77
 for domain in ['memory','performance']:
  inputs=json.loads((root/domain/'inputs.json').read_text());build=inputs['build']
  assert identity(build['fingerprint'])==build['fingerprint_sha256']
  assert build==json.loads((root/'trames-memory-build-v9.json').read_text())
  assert build['sources']['runtime']['commit']=='d544939a6dc1631dc01bd6af0b881560856c0a8c'
  assert build['fingerprint']['binary_sha256']=='c6f761bc04334e243e862083773b2e05c49791364d3462f05cede99acc622ef4'
 sources=root/'qualified-sources'
 for domain,controller in [('memory','validate-memory.py'),('performance','benchmark.py')]:
  inputs=json.loads((root/domain/'inputs.json').read_text())
  assert digest(sources/controller)==inputs['controller_sha256']
  assert inputs['host']['page_size']==4096
  if domain=='memory':
   assert inputs['load_alignments'] and set(inputs['load_alignments'])=={4096}
   assert digest(sources/'memory-4k.c')==inputs['fixture_source_sha256']
  else:
   for name,h in inputs['fixture_sources'].items():assert digest(sources/Path(name).name)==h
 stress=json.loads((root/'thread-stress/results.json').read_text())
 assert stress['passed']==stress['planned']==len(stress['cases'])==40
 assert {r['id'] for r in stress['cases']}=={f'{t}-{w}-{i}' for t in ['smccc','mmio'] for w in [2,4] for i in range(10)}
 for row in stress['cases']:
  assert row['passed'] and row['exit_code']==0
  folder=root/'thread-stress'/row['id']
  assert (folder/'stdout').read_bytes()==b'threads: ok\n' and not (folder/'stderr').read_bytes()
  for name,h in row['logs'].items():assert digest(folder/name)==h
 results['concurrent_permission_repeats']=40
 for label in ['native','kvm-smccc-1','kvm-smccc-4','kvm-mmio-1','kvm-mmio-4']:
  directory=root/('applications-native' if label=='native' else 'applications/'+label)
  plan=json.loads((directory/'plan.json').read_text());rows=json.loads((directory/'results.json').read_text())['cases']
  assert len(rows)==len(plan['cases'])==12
  by_id={r['id']:r for r in rows};assert set(by_id)=={case['id'] for case in plan['cases']}
  for case in plan['cases']:
   inputs=identity(dict(case=case,rootfs=plan['bundle_sha256'],harness=plan['harness_sha256']))
   fp=identity(dict(inputs=inputs,provider=plan['provider'],host={k:v for k,v in plan['host'].items() if k!='cpu'}))
   assert verified_evidence(directory,by_id[case['id']],case,fp),(label,case['id'])
 results['application_kvm_cells']=48;results['reused_native_application_cells']=12
 diagnostic=json.loads((root/'perf-diagnostic/results.json').read_text())
 assert diagnostic['status']=='passed' and diagnostic['instrumented'] and not diagnostic['primary_baseline']
 assert len(diagnostic['cases'])==6
 for row in diagnostic['cases']:
  assert row['passed'] and row['exit_code']==0
  for name,h in row['logs'].items():assert digest(root/'perf-diagnostic'/row['id']/name)==h
 results['separate_counter_diagnostics']=6
apple=json.loads((HERE/'apple-memory/results.json').read_text())
assert len(apple['cases'])==22 and all(r['passed'] and r['code']==0 for r in apple['cases'])
for row in apple['cases']:
 assert (HERE/'apple-memory'/(row['id']+'.stdout')).read_bytes()==(row['id'].rsplit('-',1)[0]+': ok\n').encode()
results['apple_hvf_memory_cells']=22
result=dict(passed=True,checks=results,archive_sha256=digest(HERE/'raw-evidence.tar.gz'),full_product_4k_qualified=False)
(HERE/'audit-result.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
