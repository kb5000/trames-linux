#!/usr/bin/env python3
"""Audit retained GPU VM evidence without booting a VM or running a workload."""
from pathlib import Path
import hashlib,json

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
def read(path):return json.loads(path.read_text())
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
environment=read(HERE/'environment.json')
assert environment['guest_page_bytes']==4096 and environment['gpu_accelerated']
assert not environment['nested_kvm_available'] and not environment['debug_log_enabled']
assert digest(HERE/'check-gpu.py')==environment['controller_sha256']
source=read(HERE/'fixture-source.json')
for name,value in source['fixture_sources'].items():
    assert digest(ROOT/'tests/fixtures'/name)==value
for name in ['gpu-acceptance','gpu-acceptance-reboot']:
    folder=HERE/name;result=read(folder/'results.json')
    assert result['passed']==result['planned']==3
    assert result['host_page_bytes']==4096 and not result['nested_kvm_available']
    assert {row['id'] for row in result['cases']}=={'vulkan-headless','vulkan-wayland','vulkan-xwayland-pixels'}
    assert all(row['passed'] for row in result['cases'])
    assert (folder/'vulkan-headless.stdout').read_bytes()==b'headless: ok\n'
    streams=(folder/'vulkan-wayland.stdout').read_bytes()+(folder/'vulkan-wayland.stderr').read_bytes()
    assert b'Virtio-GPU Venus (Apple M6)' in streams
    assert (folder/'pixels.stdout').read_bytes()==b'pixels: 320x240 green\n'
    assert b'wsi-ready frames=16\n' in (folder/'vulkan-xwayland-pixels.stdout').read_bytes()
assert read(HERE/'gpu-acceptance-v1/results.json')['passed']==1
software=read(HERE/'software-reference/results.json')
assert software['passed']==software['planned']==3 and not software['physical_gpu_qualified']
kvm=read(HERE/'gpu-acceptance-reboot/kvm-status.json')
assert kvm['doctor_exit_code']==2 and not kvm['nested_kvm_available']
assert kvm['runner_sha256']=='44f1f61e8e3db16e57af99f128d8a4be8566a81894cb1bf1697ac13a5e370834'
cleanup=read(HERE/'cleanup.json');general=read(HERE/'general-caches.json')
assert len(cleanup['removed'])==14 and cleanup['removed']==cleanup['plan']
assert not cleanup['source_user_data_vm_or_logs_removed'] and not general['personal_or_application_data_removed']
reclaimed=general['available_after']-cleanup['host_available_before']
assert reclaimed==environment['cleanup_reclaimed_bytes']
assert (HERE/'vulkan-desktop-final.png').read_bytes().startswith(b'\x89PNG\r\n\x1a\n')
print(json.dumps(dict(gpu=3,post_restart_gpu=3,software_reference=3,cleaned_gib=round(reclaimed/1024**3,2),kvm_unavailable=True,historical_failures_retained=True)))
