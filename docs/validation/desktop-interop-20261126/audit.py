#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Audit archived desktop interoperability evidence without KVM or images."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--source-root',type=Path)
a=p.parse_args()
ROOT=Path(__file__).resolve().parent

def read(name):return json.loads((ROOT/name).read_text())
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def identity(v):return hashlib.sha256(json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=True).encode()).hexdigest()

for name,sha in read('archive-sha256.json').items():
    assert digest(ROOT/name)==sha,name
build=read('final-build.json')
assert identity(build['fingerprint'])==build['fingerprint_sha256']
assert not build['sources']['runtime']['dirty']
assert build['sources']['runtime']['commit']=='ef0b9194a84ba10ad0952e84411d3943d0f18452'
assert 'host: aarch64-unknown-linux-gnu' in build['fingerprint']['toolchain']
main=read('qualification-final/results.json')
assert main['complete'] and main['passed'] and len(main['rows'])==32
assert not main['physical_gpu_qualified'] and not main['performance_qualified']
expected={'native-x11-vulkan-reference','native-wayland-vulkan-reference','native-x11-interop','native-x11-vulkan-shm','native-wayland-vulkan-shm'}|{'native-wayland-'+op for op in ['copy','paste','ime']}
expected|={f'kvm-{t}-{c}-{op}' for t in ['smccc','mmio'] for c in [1,4] for op in ['x11-interop','wayland-copy','wayland-paste','wayland-ime','x11-vulkan','wayland-vulkan']}
assert {r['id'] for r in main['rows']}==expected
inputs=read('qualification-final/inputs.json')
runner=build['fingerprint']['binary_sha256']
assert inputs['binary_sha256']['runner']==runner
assert inputs['runtime_pin']['commit']==build['sources']['runtime']['commit']
assert read('qualification-final/doctor.stdout')['host']['host_page_size']==4096
for row in main['rows']:
    assert row['status']=='passed',row
    directory=ROOT/'qualification-final'/row['id']
    if row['id'].endswith('-reference'):
        output=(directory/'reference/stdout').read_bytes()
    else:
        output=(directory/'guest/stdout').read_bytes()
    if row['id'].endswith('-vulkan') or row['id'].endswith('-reference'):
        assert row['original_vulkan_frames']==16 and row['swapchain_generations']==2
        assert row['pixels'] and row['teardown']
        assert b'wsi-ready frames=16\n' in output and b'wsi: ok\n' in output
    elif row['id'].endswith('x11-interop'):
        for marker in ['clipboard-sent','paste host 剪贴板 🧪','preedit 拼音','commit 中文输入 ✅']:
            assert marker.encode() in output,(row['id'],marker)
    elif row['id'].endswith('wayland-copy'):
        assert 'paste guest 剪贴板 ✅'.encode() in (directory/'peer/stdout').read_bytes()
    elif row['id'].endswith('wayland-paste'):
        assert 'paste host 剪贴板 🧪'.encode() in output
    elif row['id'].endswith('wayland-ime'):
        for marker in ['preedit 拼音','delete 3 0','commit 中文输入 ✅']:
            assert marker.encode() in output
    elif row['id']=='native-x11-vulkan-shm':
        assert row['input'] and row['resize'] and row['close'] and len(row['pixels'])==5
    elif row['id']=='native-wayland-vulkan-shm':
        assert row['renderer']=='vulkan' and b'key 30 1\n' in output and b'frame ' in output
xim=read('xwayland-ime-final/results.json')
assert xim['complete'] and xim['passed'] and len(xim['rows'])==4
assert {r['id'] for r in xim['rows']}=={f'xwayland-ime-{t}-{c}' for t in ['smccc','mmio'] for c in [1,4]}
assert read('xwayland-ime-final/inputs.json')['binaries']['runner']==runner
for row in xim['rows']:
    assert row['status']=='passed' and row['original_xlib'] and row['utf8_commit']
    assert 'guest-xim-commit 中文输入 ✅\n'.encode() in (ROOT/'xwayland-ime-final'/row['id']/'guest/stdout').read_bytes()
legacy=read('desktop-regression-final/results.json')
assert legacy['complete'] and legacy['passed'] and len(legacy['rows'])==13
assert legacy['inputs']['runner_sha256']==runner
assert all(r['status']=='passed' for r in legacy['rows'])
assert inputs['binary_sha256']['unit_binary']==legacy['inputs']['unit_sha256']
icd=read('guest-icd-source.json')
assert icd['source_matches_commit'] and icd['icd_sha256']==inputs['binary_sha256']['guest_icd']
for name,sha in inputs['sources'].items():
    path=ROOT/'sources'/name
    if path.is_file():assert digest(path)==sha,name
for cohort in ['xwayland-ime-final','desktop-regression-final']:
    data=read(cohort+'/inputs.json') if cohort=='xwayland-ime-final' else legacy['inputs']
    for name,sha in data['sources'].items():
        path=ROOT/'sources'/name
        if path.is_file():assert digest(path)==sha,name
if a.source_root:
    for name,sha in inputs['sources'].items():
        assert digest(a.source_root/name)==sha,name
    spec=importlib.util.spec_from_file_location('acceptance_common',a.source_root/'runtime/tests/acceptance/common.py')
    common=importlib.util.module_from_spec(spec);spec.loader.exec_module(common)
    for name,path in [('product',a.source_root),('runtime',a.source_root/'runtime')]:
        assert common.source_identity(path)['inputs_sha256']==build['sources'][name]['inputs_sha256'],name
cloud=read('cloud-checks.json') if (ROOT/'cloud-checks.json').exists() else {}
if cloud.get('complete'):
    assert cloud['sources']['runtime']==build['sources']['runtime']['commit']
    assert cloud['sources']['product']=='b80f046d56d822dd1d142450459684b0f963920b'
    for group in [cloud['runtime']['runs'],cloud['product']['runs'],cloud['runtime']['jobs'],cloud['product']['build_jobs'],cloud['product']['native_jobs']]:
        assert group and all(r['status']=='completed' and r['conclusion']=='success' for r in group)
    assert len(cloud['product']['build_jobs'])==3 and len(cloud['product']['native_jobs'])==2
    assert len(cloud['native_interop_log_evidence'])==2
    for evidence in cloud['native_interop_log_evidence']:
        assert sum(': passed' in line for line in evidence['lines'])==8
        assert any('"cases": 8' in line for line in evidence['lines'])
for filename,count in [('linux-interop-runtime-tests.log',97),('linux-interop-product-tests.log',23)]:
    import re
    text=(ROOT/filename).read_text()
    assert 'test result: FAILED' not in text
    assert sum(map(int,re.findall(r'test result: ok\. (\d+) passed;',text)))==count,filename
result=dict(passed=True,new_interop_cells=32,guest_xwayland_ime_cells=4,desktop_regressions=13,runtime_unit_passes=97,product_unit_passes=23,runner_sha256=runner,physical_gpu_qualified=False,performance_qualified=False,cloud_checks_complete=cloud.get('complete',False))
(ROOT/'audit-result.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result))
