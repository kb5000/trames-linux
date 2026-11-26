#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Original ARM64 CPU extensions/state/ptrace: native vs real 4KiB KVM."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'runtime/tests/acceptance'))
from common import digest, save, tree_digest
from run import execute, host, receipt


def dependencies(source, root, copied):
    for line in subprocess.check_output(['ldd',source],text=True).splitlines():
        if 'not found' in line:raise RuntimeError('missing original ELF dependency: '+line)
        for item in line.split():
            if not item.startswith('/') or item in copied:continue
            copied.add(item);target=root/item.lstrip('/');target.parent.mkdir(parents=True,exist_ok=True)
            shutil.copy2(item,target);dependencies(item,root,copied)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runner',type=Path,required=True)
    parser.add_argument('--receipt',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    a=parser.parse_args();a.output=a.output.resolve();a.output.mkdir(parents=True,exist_ok=False)
    runner=a.runner.resolve();build=receipt(a.receipt.resolve(),runner);out=a.output
    doctor=subprocess.run([runner,'doctor','--json','--require-kvm'],capture_output=True,timeout=15)
    (out/'doctor.stdout').write_bytes(doctor.stdout);(out/'doctor.stderr').write_bytes(doctor.stderr)
    if doctor.returncode:raise RuntimeError('real KVM CPU preflight failed')
    profile=json.loads(doctor.stdout)['host']['cpu'];assert profile['architecture']=='aarch64'
    source=ROOT/'tests/fixtures/cpu-extensions.c';binary=out/'original-cpu-elf'
    command=[os.environ.get('CC','cc'),'-O2','-Wall','-Wextra','-Werror','-march=armv8.6-a+crypto+sha3+sm4+fp16','-fno-pie','-no-pie','-pthread',str(source),'-o',str(binary)]
    with (out/'fixture.build.log').open('wb') as log:subprocess.run(command,check=True,stdout=log,stderr=subprocess.STDOUT)
    root=out/'rootfs';(root/'test').mkdir(parents=True);shutil.copy2(binary,root/'test/cpu')
    dependencies(binary,root,set());(root/'tmp').mkdir(mode=0o1777)
    save(out/'inputs.json',dict(build=build,controller_sha256=digest(Path(__file__)),fixture_source_sha256=digest(source),fixture_binary_sha256=digest(binary),rootfs_sha256=tree_digest(root),compiler_command=command,profile=profile,host=host(),performance_qualified=False))
    rows=[];native={};mask=[hex(profile['hwcap']),hex(profile['hwcap2'])]
    for mode in ['instructions','state','ptrace']:
        folder=out/('native-'+mode);folder.mkdir();args=[mode]+(mask if mode=='instructions' else [])
        result=execute([binary,*args],os.environ.copy(),folder,60)
        passed=result['status']=='completed' and result['exit_code']==0
        if mode=='instructions':passed &= (folder/'stdout').read_bytes().endswith(b'instructions: ok\n')
        elif mode=='state':passed &= (folder/'stdout').read_bytes()==b'state: 4 threads, 256 signal frames, nonzero FPCR/FPSR ok\n'
        else:passed &= (folder/'stdout').read_bytes()==b'ptrace: original instructions and syscall single-step ok\n'
        native[mode]=passed
        rows.append(dict(id='native-'+mode,**result)|dict(status='passed' if passed else 'failed'))
        print(rows[-1],flush=True)
    for transport in ['smccc','mmio']:
        for workers in [1,4]:
            for mode in ['instructions','state','ptrace']:
                name=f'{mode}-{transport}-{workers}';folder=out/name;folder.mkdir()
                if not native[mode]:rows.append(dict(id=name,status='reference_failed'));continue
                scratch=out/(name+'-rootfs');shutil.copytree(root,scratch)
                try:
                    args=[mode]+([*mask,profile['features']] if mode=='instructions' else [])
                    command=[runner,'run','--cpus',str(workers),'--rootfs',scratch,'--timeout-ms','60000','--','/test/cpu',*args]
                    result=execute(command,os.environ|{'TRAMES_KVM_TRANSPORT':transport},folder,65)
                    passed=result['status']=='completed' and result['exit_code']==0 and (folder/'stdout').read_bytes()==(out/('native-'+mode)/'stdout').read_bytes()
                    rows.append(dict(id=name,**result)|dict(status='passed' if passed else result['status'] if result['status']!='completed' else 'failed'))
                    print(rows[-1],flush=True)
                finally:shutil.rmtree(scratch)
    result=dict(cases=rows,passed=sum(r['status']=='passed' for r in rows),planned=len(rows),performance_qualified=False)
    save(out/'results.json',result)
    return 0 if result['passed']==result['planned']==15 else 1


if __name__=='__main__':raise SystemExit(main())
