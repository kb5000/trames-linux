#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Audit frozen keyboard/desktop evidence without KVM or VM images."""
import argparse,hashlib,importlib.util,json,re
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--source-root',type=Path);a=p.parse_args()
ROOT=Path(__file__).resolve().parent
PIN='16e8ed40f2939cca1888a7d7c4a00224b5b8edb4'
def read(name):return json.loads((ROOT/name).read_text())
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def identity(v):return hashlib.sha256(json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=True).encode()).hexdigest()
for name,sha in read('archive-sha256.json').items():assert digest(ROOT/name)==sha,name
build=read('release-build.json');assert identity(build['fingerprint'])==build['fingerprint_sha256']
assert build['sources']['runtime']['commit']==PIN and not build['sources']['runtime']['dirty']
assert 'host: aarch64-unknown-linux-gnu' in build['fingerprint']['toolchain']
runner=build['fingerprint']['binary_sha256'];unit=None
expected={'reference-x11','reference-wayland','native-x11','native-wayland'}
expected|={f'kvm-{t}-{c}-host-{h}-{g}' for t in ['smccc','mmio'] for c in [1,4] for h in ['x11','wayland'] for g in ['wayland','xwayland']}
keyboard=read('keyboard-release/results.json');inputs=read('keyboard-release/inputs.json')
assert keyboard['complete'] and keyboard['passed'] and len(keyboard['rows'])==20
assert {r['id'] for r in keyboard['rows']}==expected and not keyboard['physical_gpu_qualified']
assert inputs['binaries']['runner']==runner and inputs['runtime_pin']['commit']==PIN
unit=inputs['binaries']['unit_binary']
assert read('keyboard-release/doctor.json')['host']['host_page_size']==4096
assert read('keyboard-release/doctor.json')['runtime']['commit']==PIN
for r in keyboard['rows']:
 assert r['status']=='passed' and all(r[k] for k in ['german','french','russian_group','altgr','caps_across_map_change','numlock','focus_release'])
 assert r['latched_shift']==('host-x11' in r['id'] or r['id'].endswith('-x11'))
 x11=r['id']=='reference-x11' or r['id'].endswith('-xwayland')
 assert (r['physical_down_events_during_hold']>=2 if x11 else r['physical_down_events_during_hold']==1)
 output=(ROOT/'keyboard-release'/r['id']/'client/stdout').read_text()
 assert 'keyboard-done' in output and 'text=@' in output and 'text=й' in output and 'text=z' in output
interop=read('interop-release/results.json');inter_inputs=read('interop-release/inputs.json')
assert interop['complete'] and interop['passed'] and len(interop['rows'])==32
assert inter_inputs['binary_sha256']['runner']==runner and inter_inputs['binary_sha256']['unit_binary']==unit
assert inter_inputs['runtime_pin']['commit']==PIN and not interop['physical_gpu_qualified'] and not interop['performance_qualified']
expected={'native-x11-vulkan-reference','native-wayland-vulkan-reference','native-x11-interop','native-x11-vulkan-shm','native-wayland-vulkan-shm'}|{'native-wayland-'+op for op in ['copy','paste','ime']}
expected|={f'kvm-{t}-{c}-{op}' for t in ['smccc','mmio'] for c in [1,4] for op in ['x11-interop','wayland-copy','wayland-paste','wayland-ime','x11-vulkan','wayland-vulkan']}
assert {r['id'] for r in interop['rows']}==expected
for r in interop['rows']:
 assert r['status']=='passed'
 directory=ROOT/'interop-release'/r['id'];output=(directory/('reference' if r['id'].endswith('-reference') else 'guest')/'stdout').read_text()
 if r['id'].endswith(('-vulkan','-reference')):assert r['original_vulkan_frames']==16 and r['swapchain_generations']==2 and r['teardown'] and 'wsi: ok' in output
 elif r['id'].endswith('x11-interop'):assert all(t in output for t in ['paste host 剪贴板 🧪','preedit 拼音','commit 中文输入 ✅'])
 elif r['id'].endswith('wayland-ime'):assert all(t in output for t in ['preedit 拼音','delete 3 0','commit 中文输入 ✅'])
 elif r['id'].endswith('wayland-paste'):assert 'paste host 剪贴板 🧪' in output
 elif r['id'].endswith('wayland-copy'):assert 'paste guest 剪贴板 ✅' in (directory/'peer/stdout').read_text()
xim=read('xim-release/results.json');assert xim['complete'] and xim['passed'] and len(xim['rows'])==4
assert {r['id'] for r in xim['rows']}=={f'xwayland-ime-{t}-{c}' for t in ['smccc','mmio'] for c in [1,4]}
assert read('xim-release/inputs.json')['binaries']['runner']==runner
for r in xim['rows']:assert r['status']=='passed' and 'guest-xim-commit 中文输入 ✅' in (ROOT/'xim-release'/r['id']/'guest/stdout').read_text()
legacy=read('desktop-release/results.json');assert legacy['complete'] and legacy['passed'] and len(legacy['rows'])==13
assert all(r['status']=='passed' for r in legacy['rows'])
assert legacy['inputs']['runner_sha256']==runner and legacy['inputs']['unit_sha256']==unit
assert inter_inputs['binary_sha256']['guest_icd']==read('guest-icd-source.json')['icd_sha256']
for name,count in [('runtime-tests-final.log',29),('product-tests-final2.log',23),('console-tests-linux.log',1),('console-apple.log',1)]:
 text=(ROOT/name).read_text();assert 'test result: FAILED' not in text
 assert sum(map(int,re.findall(r'test result: ok\. (\d+) passed;',text)))==count,name
for name in ['shared-linux-clippy.log','shared-apple-clippy.log','apple-clippy.log','clippy-final2.log']:
 text=(ROOT/name).read_text();assert 'Finished `dev`' in text and '\nerror:' not in text,name
for data in [inputs,inter_inputs,read('xim-release/inputs.json'),legacy['inputs']]:
 for name,sha in data['sources'].items():
  snapshot=ROOT/'sources'/name
  if snapshot.is_file():assert digest(snapshot)==sha,name
  if a.source_root:assert digest(a.source_root/name)==sha,name
if a.source_root:
 spec=importlib.util.spec_from_file_location('acceptance_common',a.source_root/'runtime/tests/acceptance/common.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
 for name,path in [('product',a.source_root),('runtime',a.source_root/'runtime')]:assert m.source_identity(path)['inputs_sha256']==build['sources'][name]['inputs_sha256'],name
cloud=read('cloud-checks.json') if (ROOT/'cloud-checks.json').exists() else {}
if cloud.get('complete'):
 assert cloud['runtime_pin']==PIN
 for name in ['runtime_runs','product_runs','runtime_jobs','build_jobs','native_jobs']:
  assert cloud[name] and all(r['status']=='completed' and r['conclusion']=='success' for r in cloud[name]),name
 assert len(cloud['build_jobs'])==3 and len(cloud['native_jobs'])==2
 assert len(cloud['keyboard_log_evidence'])==2
 for item in cloud['keyboard_log_evidence']:
  assert sum(': passed' in line for line in item['lines'])==4 and any('"cases": 4' in line for line in item['lines'])
result=dict(passed=True,keyboard_cells=20,interop_cells=32,xlib_ime_cells=4,desktop_cells=13,runner_sha256=runner,runtime_pin=PIN,physical_gpu_qualified=False,cloud_checks_complete=cloud.get('complete',False))
(ROOT/'audit-result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
