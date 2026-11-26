#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Audit archived software desktop evidence without KVM or original images."""
import argparse
import hashlib
import json
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--source-root',type=Path);a=p.parse_args()
ROOT=Path(__file__).resolve().parent
def read(path):return json.loads((ROOT/path).read_text())
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def identity(value):return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=True).encode()).hexdigest()
archive=read('archive-sha256.json')
for name,expected in archive.items():assert digest(ROOT/name)==expected,name
final=read('linux-desktop-final/results.json');expected={'native-wayland','store-create','store-reopen','terminal-weston','terminal-xterm'}|{f'kvm-{t}-{c}-{w}' for t in ['smccc','mmio'] for c in [1,4] for w in ['wayland','xwayland']}
assert final['passed'] and final['complete'] and len(final['rows'])==13
assert {r['id'] for r in final['rows']}==expected
for row in final['rows']:
 assert row['status']=='passed'
 path=ROOT/'linux-desktop-final'/('native' if row['id']=='native-wayland' else row['id'])
 output=(path/'stdout').read_bytes()
 if row['id'].startswith('terminal-'):
  assert row['keyboard_shell_command'] and row['cli_survives'] and b'guiok\n' in output and b'cli-survived\n' in output
 else:
  assert row['input'] and row['focus_loss'] and row['resize'] and row['expose'] and row['close']
  assert len(row['pixels'])==5 and row['pixels'][0]=='pixels 240 160 20c040' and row['pixels'][-1]=='pixels 320 200 2040c0'
  assert 0<=row['idle_cpu_seconds']<=.25
  assert b'key 30 1\n' in output and b'key 30 0\n' in output and b'desktop-done\n' in output
  if 'xwayland' not in row['id'] and row['id']!='store-reopen':assert b'frame 3\n' in output and b'focus-in keys=0\n' in output
assert read('linux-desktop-final/doctor.json')['host']['host_page_size']==4096
final_build=read('linux-desktop-final-build.json');headless_build=read('linux-desktop-build.json')
for build in [final_build,headless_build]:assert identity(build['fingerprint'])==build['fingerprint_sha256']
assert final['inputs']['runner_sha256']==final_build['fingerprint']['binary_sha256']
assert final_build['sources']['runtime']==headless_build['sources']['runtime']
assert final['inputs']['runtime_pin']['commit']==final_build['sources']['runtime']['commit']=='fd1463fea2aa46b691fbdb62e686e00473debf0a'
for name,expected_hash in final['inputs']['sources'].items():assert digest(ROOT/'sources'/name)==expected_hash
assert digest(ROOT/'sources/scripts/validate-desktop.py')==final['inputs']['controller_sha256']
old=read('linux-desktop-source-manifest.json');new=read('linux-desktop-final-source-manifest.json')
different={n for n in old.keys()|new.keys() if old.get(n)!=new.get(n)}
assert different=={'crates/trames-linux-app/src/foreground.rs','scripts/validate-desktop.py','tests/fixtures/desktop-probe.c'},different
# The production difference after the headless cohort is inside host.desktop.
text=(ROOT/'sources/crates/trames-linux-app/src/foreground.rs').read_text();start=text.index('        // GUI terminals allocate their own PTYs.');end=text.index('        let server =',start)
assert text.rfind('if host.desktop',0,start)>0
reconstructed=text[:start]+text[end:]
assert hashlib.sha256(reconstructed.encode()).hexdigest()==old['crates/trames-linux-app/src/foreground.rs']
assert digest(ROOT/'linux-desktop-headless-foreground.rs')==old['crates/trames-linux-app/src/foreground.rs']
memory=read('linux-desktop-memory/results.json');assert memory['passed']==memory['planned']==77 and len(memory['cases'])==77 and all(r['status']=='passed' for r in memory['cases'])
apps=[]
for transport in ['smccc','mmio']:
 for cpus in [1,4]:
  d=read(f'linux-desktop-applications/kvm-{transport}-{cpus}/results.json')
  assert len(d['cases'])==12 and d['counts']['passed']==12 and all(r['status']=='passed' for r in d['cases']);apps.extend(d['cases'])
assert len(apps)==48
for name in ['checks/trames-desktop-distro-mac.log','checks/linux-desktop-distro-linux.log']:assert '42 passed; 0 failed' in (ROOT/name).read_text()
for name in ['checks/trames-desktop-mac-test-complete.log','checks/linux-desktop-tests-complete.log']:
 text=(ROOT/name).read_text();assert 'test result: FAILED' not in text and '; 0 failed;' in text
metadata=read('published-source.json')['metadata_file_changes']
assert set(metadata)=={'third-party/inputs.json'}
entry=metadata['third-party/inputs.json'];assert entry['old_sha256']==new['third-party/inputs.json']
assert hashlib.sha256(entry['new_contents'].encode()).hexdigest()==entry['new_sha256']
assert json.loads(entry['new_contents'])=={'Cargo.lock':new['Cargo.lock']}
if a.source_root:
 for name,expected_hash in new.items():
  expected_hash=metadata.get(name,{}).get('new_sha256',expected_hash)
  assert digest(a.source_root/name)==expected_hash,name
cloud={r['label']:r for r in read('cloud-checks.json')}
for label in ['runtime','runtime-notices','native-presentation','notices-corrected','macos-release-only']:
 assert cloud[label]['jobs'] and all(j['status']=='completed' and j['conclusion']=='success' for j in cloud[label]['jobs']),label
initial=cloud['linux-build-initial']['jobs']
assert len(initial)==3 and all(j['conclusion']=='success' for j in initial if 'ubuntu' in j['name'])
mac=next(j for j in initial if 'macos' in j['name'])
assert mac['conclusion']=='cancelled'
steps={s['name']:s for s in mac['steps']}
for name in ['Run cargo clippy --workspace --all-targets --locked -- -D warnings','Run cargo test --workspace --locked']:assert steps[name]['conclusion']=='success'
assert steps['Run cargo build --workspace --release --locked']['conclusion']=='cancelled'
retry=cloud['macos-release-only']['jobs'];assert len(retry)==1 and 'macos' in retry[0]['name']
steps={s['name']:s for s in retry[0]['steps']}
for name in ['Run cargo clippy --workspace --all-targets --locked -- -D warnings','Run cargo test --workspace --locked']:assert steps[name]['conclusion']=='skipped'
for name in ['Run cargo build --workspace --release --locked','Run target/release/trames doctor --json']:assert steps[name]['conclusion']=='success'
for j in cloud['native-presentation']['jobs']:
 assert any(s['name']=='Exercise native SHM desktop pixels and keyboard/pointer input' and s['conclusion']=='success' for s in j['steps'])
result=dict(passed=True,cloud_ci_closed=True,archived_files=len(archive),desktop=13,memory=77,applications=48,runtime=final_build['sources']['runtime']['commit'],final_runner_sha256=final['inputs']['runner_sha256'],headless_runner_sha256=headless_build['fingerprint']['binary_sha256'],physical_gpu_qualified=False)
(ROOT/'audit-result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
