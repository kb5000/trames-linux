#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Check frozen display cohorts, exact-source receipt, and retained pixel observations."""
import argparse,hashlib,importlib.util,json,re,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--source-root',type=Path);a=p.parse_args();ROOT=Path(__file__).resolve().parent
PIN='4e59e92504331039250bdb90b698ff1bf79b98c9'
def read(n):return json.loads((ROOT/n).read_text())
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def identity(v):return hashlib.sha256(json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=True).encode()).hexdigest()
for name,sha in read('archive-sha256.json').items():assert digest(ROOT/name)==sha,name
for name,sha in read('source-sha256.json').items():
 assert digest(ROOT/'sources'/name)==sha,name
 if a.source_root:assert digest(a.source_root/name)==sha,name
build=read('build.json');assert identity(build['fingerprint'])==build['fingerprint_sha256'];assert build['sources']['runtime']['commit']==PIN and not build['sources']['runtime']['dirty'];assert 'host: aarch64-unknown-linux-gnu' in build['fingerprint']['toolchain']
sha=build['fingerprint']['binary_sha256'];unit=read('matrix/inputs.json')['binaries']['unit_binary']
assert read('matrix/inputs.json')['binaries']['runner']==sha
assert read('windows/inputs.json')['binaries']==dict(runner=sha,unit_binary=unit,launcher=read('windows/inputs.json')['binaries']['launcher'])
assert read('desktop/results.json')['inputs']['runner_sha256']==sha and read('desktop/results.json')['inputs']['unit_sha256']==unit
assert read('interop/inputs.json')['binary_sha256']['runner']==sha and read('interop/inputs.json')['binary_sha256']['unit_binary']==unit
counts={}
for name,count in [('matrix',13),('windows',20),('desktop',13),('interop',32)]:
 result=read(name+'/results.json');assert result['complete'] and result['passed'];assert len(result['rows'])==count and all(r['status']=='passed' for r in result['rows']);assert len({r['id'] for r in result['rows']})==count;counts[name]=count
expected={'reference-wayland'}|{f'native-{h}-{r}' for h in ['x11','wayland'] for r in ['software','vulkan']}|{f'kvm-{t}-{c}-{h}' for t in ['smccc','mmio'] for c in [1,4] for h in ['x11','wayland']}
assert {r['id'] for r in read('matrix/results.json')['rows']}==expected
for row in read('matrix/results.json')['rows']:
 assert not row['physical_gpu_qualified'];text=(ROOT/'matrix'/row['id']/'client/stdout').read_text();assert 'hidpi-done' in text
 if row['host']=='wayland':
  assert read('matrix/'+row['id']+'/fullscreen-target.json')['output']=='HEADLESS-2'
  assert len(row['screenshots'])==4
  for s,n in zip(row['screenshots'],[240,300,360,480]):assert s['alternating_run']>=n-2
  for marker in ['pixels=300,200 scale=150','pixels=360,240 scale=180','pixels=480,320 scale=240','transform=3','removed HEADLESS-2','pointer 120.00 80.00']:assert marker in text,marker
 else:
  for marker in ['pixels=300,200 scale=150','pixels=400,250 scale=150','removed RIGHT','pointer 120.00 80.00']:assert marker in text,marker
assert read('matrix/doctor.json')['host']['host_page_size']==4096 and read('matrix/doctor.json')['runtime']['commit']==PIN
for path,expected_count in [('shared-tests-linux.log',37),('shared-tests-apple.log',38),('product-tests-linux.log',26),('main-tests-linux.log',40),('main-tests-apple.log',41)]:
 text=(ROOT/path).read_text();assert 'test result: FAILED' not in text and sum(map(int,re.findall(r'test result: ok\. (\d+) passed;',text)))==expected_count,path
for path in ['clippy-linux.log','shared-clippy-apple.log','check-apple.log','main-clippy-apple.log']:
 text=(ROOT/path).read_text();assert 'Finished `dev`' in text and '\nerror:' not in text,path
if a.source_root:
 spec=importlib.util.spec_from_file_location('common',a.source_root/'runtime/tests/acceptance/common.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
 for name,path in [('product',a.source_root),('runtime',a.source_root/'runtime')]:assert m.source_identity(path)['inputs_sha256']==build['sources'][name]['inputs_sha256'],name
 for cohort in ['matrix','windows','interop']:
  for name,sha0 in read(cohort+'/inputs.json')['sources'].items():assert digest(a.source_root/name)==sha0,(cohort,name)
 subprocess.run(['git','merge-base','--is-ancestor',PIN,'origin/main'],cwd=a.source_root/'runtime',check=True)
cloud=read('cloud-checks.json') if (ROOT/'cloud-checks.json').exists() else {}
if cloud.get('complete'):
 assert len(cloud['build_jobs'])==3 and len(cloud['native_jobs'])==2
 assert all(v['conclusion']=='success' for name in ['product_runs','build_jobs','native_jobs'] for v in cloud[name])
result=dict(passed=True,display_cells=sum(counts.values()),cohorts=counts,runtime_pin=PIN,runner_sha256=sha,cloud_checks_complete=cloud.get('complete',False))
(ROOT/'audit-result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
