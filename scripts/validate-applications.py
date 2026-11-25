#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""One offline software acceptance matrix, using the shared runtime harness."""
import argparse
import shutil
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[1]
HARNESS=ROOT/'runtime/tests/acceptance'


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--bundle',type=Path,required=True)
    p.add_argument('--runner',type=Path,required=True)
    p.add_argument('--receipt',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--reuse',type=Path)
    p.add_argument('--cpus',type=int,nargs='+',choices=range(1,33),default=[1,4])
    p.add_argument('--transport',choices=['smccc','mmio'],nargs='+',default=['smccc','mmio'])
    p.add_argument('--manifest',type=Path)
    p.add_argument('--network',choices=['off','loopback','outbound','full'])
    p.add_argument('--filesystem',choices=['host','store'],default='host')
    p.add_argument('--native-container',type=Path)
    p.add_argument('--case',action='append')
    p.add_argument('--reference',type=Path,help='Reuse an existing exact native reference instead of running sudo/native')
    a=p.parse_args()
    if len(set(a.cpus))!=len(a.cpus) or len(set(a.transport))!=len(a.transport):p.error('duplicate execution configurations')
    a.output.mkdir(parents=True,exist_ok=False)
    # Freeze the entire matrix before native setup. Later builds/edits must not
    # switch the binary, helper or manifest between execution configurations.
    shutil.copytree(HARNESS,a.output/'harness',ignore=shutil.ignore_patterns('__pycache__','*.pyc'))
    harness=(a.output/'harness').resolve()
    sys.path.insert(0,str(harness))
    from run import receipt,freeze_runner
    build=receipt(a.receipt.resolve(),a.runner.resolve())
    a.runner=freeze_runner(a.runner.resolve(),build['fingerprint']['binary_sha256'],a.output).resolve()
    saved_receipt=a.output/'build.json';shutil.copyfile(a.receipt,saved_receipt);a.receipt=saved_receipt.resolve()
    manifest=a.manifest or harness/'cli.json'
    shutil.copyfile(manifest,a.output/'manifest.json')
    common=['--bundle',str(a.bundle.resolve()),'--manifest',str((a.output/'manifest.json').resolve())]
    for name in a.case or []:common.extend(['--case',name])
    native=a.reference.resolve() if a.reference else (a.output/'native').resolve()
    labels=[('native',native)];failed=False
    if not a.reference:
        command=['sudo','-n',sys.executable,str(harness/'run.py'),'--backend','native',*common,'--output',str(native)]
        if a.native_container:command.extend(['--native-container',str(a.native_container.resolve())])
        if a.reuse and (a.reuse/'native/results.json').exists():command.extend(['--reuse',str((a.reuse/'native').resolve())])
        result=subprocess.run(command)
        if result.returncode==2:return 2
        failed|=result.returncode!=0
    for transport in a.transport:
        for cpus in a.cpus:
            label=f'kvm-{transport}-{cpus}';output=(a.output/label).resolve();labels.append((label,output))
            command=[sys.executable,str(harness/'run.py'),'--backend','kvm',*common,'--output',str(output),'--runner',str(a.runner.resolve()),'--receipt',str(a.receipt.resolve()),'--reference',str(native),'--transport',transport,'--filesystem',a.filesystem,'--cpus',str(cpus)]
            if a.network:command.extend(['--network',a.network])
            if a.reuse and (a.reuse/label/'results.json').exists():command.extend(['--reuse',str((a.reuse/label).resolve())])
            result=subprocess.run(command)
            failed|=result.returncode!=0
    command=[sys.executable,str(harness/'report.py'),'--output',str(a.output/'report')]
    for label,path in labels:command.extend(['--input',label+'='+str(path)])
    result=subprocess.run(command)
    return 1 if failed or result.returncode else 0


if __name__=='__main__':raise SystemExit(main())
