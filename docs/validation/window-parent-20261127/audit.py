#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Audit the isolated parent-error correction and its preserved normal-flow cohort."""
import argparse,hashlib,importlib.util,json,re,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--source-root',type=Path);a=p.parse_args();ROOT=Path(__file__).resolve().parent
PIN='7b4eae68b12a480460b2ca71145826b59f7e4dfb'
def read(n):return json.loads((ROOT/n).read_text())
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def identity(v):return hashlib.sha256(json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=True).encode()).hexdigest()
for name,sha in read('archive-sha256.json').items():assert digest(ROOT/name)==sha,name
build=read('build.json');assert identity(build['fingerprint'])==build['fingerprint_sha256'];assert build['sources']['runtime']['commit']==PIN and not build['sources']['runtime']['dirty'];assert 'host: aarch64-unknown-linux-gnu' in build['fingerprint']['toolchain']
inputs=read('matrix/inputs.json');assert inputs['runtime_pin']['commit']==PIN and inputs['binaries']['runner']==build['fingerprint']['binary_sha256']
result=read('matrix/results.json');assert result['complete'] and result['passed'] and not result['physical_gpu_qualified']
expected={'native-x11','native-wayland'}|{f'kvm-{t}-{c}-{h}' for t,c in [('smccc',1),('mmio',4)] for h in ['x11','wayland']}
assert len(result['rows'])==6 and {r['id'] for r in result['rows']}==expected
for r in result['rows']:
 assert r['status']=='passed' and r['protocol_error']==1 and r['interface']=='xdg_toplevel'
 assert 'parent-error code=1 interface=xdg_toplevel' in (ROOT/'matrix'/r['id']/'client/stdout').read_text()
assert result['baseline']['status']=='failed-as-expected' and result['baseline']['incorrect_error']==3 and result['baseline']['return_code']!=0
assert 'parent-error code=3 interface=xdg_toplevel' in (ROOT/'matrix/baseline-invalid-code/client/stdout').read_text()
old=ROOT.parent/'windows-20261127';old_inputs=json.loads((old/'windows-release/inputs.json').read_text());assert inputs['binaries']['baseline_unit']==old_inputs['binaries']['unit_binary']
reuse=read('normal-path-reuse.json');assert reuse['runtime_pin']==PIN and reuse['changed_runtime_paths']==['crates/trames-wayland/src/shell.rs']
before=(old/'sources/runtime/crates/trames-wayland/src/shell.rs').read_text();after=(ROOT/'sources/runtime/crates/trames-wayland/src/shell.rs').read_text()
replacement='r.post_error(\n                        xdg_toplevel::Error::InvalidParent as u32,\n                        "cyclic window parent",\n                    );'
assert after==before.replace('r.post_error(3u32, "cyclic window parent");',replacement)
for name,sha in inputs['sources'].items():
 assert digest(ROOT/'sources'/name)==sha,name
 if a.source_root:assert digest(a.source_root/name)==sha,name
for name in ['shared-linux.log','shared-apple.log']:
 text=(ROOT/name).read_text();assert 'Finished `dev`' in text and '\nerror:' not in text
text=(ROOT/'shared-tests.log').read_text();assert sum(map(int,re.findall(r'test result: ok\. (\d+) passed;',text)))==33 and 'test result: FAILED' not in text
assert read('matrix/doctor.json')['runtime']['commit']==PIN and read('matrix/doctor.json')['host']['host_page_size']==4096
if a.source_root:
 spec=importlib.util.spec_from_file_location('common',a.source_root/'runtime/tests/acceptance/common.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
 for name,path in [('product',a.source_root),('runtime',a.source_root/'runtime')]:assert m.source_identity(path)['inputs_sha256']==build['sources'][name]['inputs_sha256'],name
 changed=subprocess.check_output(['git','diff','--name-only',reuse['baseline_runtime'],PIN],cwd=a.source_root/'runtime',text=True).splitlines();assert changed==reuse['changed_runtime_paths']
 subprocess.run(['git','merge-base','--is-ancestor',PIN,'origin/main'],cwd=a.source_root/'runtime',check=True)
subprocess.run(['python3',str(old/'audit.py')],check=True,stdout=subprocess.DEVNULL)
cloud=read('cloud-checks.json') if (ROOT/'cloud-checks.json').exists() else {}
if cloud.get('complete'):
 assert cloud['runtime_pin']==PIN
 for key in ['product_runs','build_jobs','native_jobs']:assert cloud[key] and all(x['status']=='completed' and x['conclusion']=='success' for x in cloud[key])
 assert len(cloud['build_jobs'])==3 and len(cloud['native_jobs'])==2 and len(cloud['parent_log_evidence'])==2
 for item in cloud['parent_log_evidence']:assert sum(': passed' in x for x in item['lines'])==2 and any('"cases": 2' in x for x in item['lines'])
report=dict(passed=True,parent_protocol_cells=6,old_error_reproduced=True,reused_normal_cells=89,runtime_pin=PIN,runner_sha256=build['fingerprint']['binary_sha256'],cloud_checks_complete=cloud.get('complete',False))
(ROOT/'audit-result.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
