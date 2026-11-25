#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""One offline software acceptance matrix, using the shared runtime harness."""
import argparse
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
    p.add_argument('--case',action='append')
    p.add_argument('--reference',type=Path,help='Reuse an existing exact native reference instead of running sudo/native')
    a=p.parse_args()
    if len(set(a.cpus))!=len(a.cpus) or len(set(a.transport))!=len(a.transport):p.error('duplicate execution configurations')
    a.output.mkdir(parents=True,exist_ok=False)
    common=['--bundle',str(a.bundle.resolve())]
    for name in a.case or []:common.extend(['--case',name])
    native=a.reference.resolve() if a.reference else (a.output/'native').resolve()
    labels=[('native',native)];failed=False
    if not a.reference:
        command=['sudo','-n',sys.executable,str(HARNESS/'run.py'),'--backend','native',*common,'--output',str(native)]
        if a.reuse and (a.reuse/'native/results.json').exists():command.extend(['--reuse',str((a.reuse/'native').resolve())])
        result=subprocess.run(command)
        if result.returncode==2:return 2
        failed|=result.returncode!=0
    for transport in a.transport:
        for cpus in a.cpus:
            label=f'kvm-{transport}-{cpus}';output=(a.output/label).resolve();labels.append((label,output))
            command=[sys.executable,str(HARNESS/'run.py'),'--backend','kvm',*common,'--output',str(output),'--runner',str(a.runner.resolve()),'--receipt',str(a.receipt.resolve()),'--reference',str(native),'--transport',transport,'--cpus',str(cpus)]
            if a.reuse and (a.reuse/label/'results.json').exists():command.extend(['--reuse',str((a.reuse/label).resolve())])
            result=subprocess.run(command)
            failed|=result.returncode!=0
    command=[sys.executable,str(HARNESS/'report.py'),'--output',str(a.output/'report')]
    for label,path in labels:command.extend(['--input',label+'='+str(path)])
    result=subprocess.run(command)
    return 1 if failed or result.returncode else 0


if __name__=='__main__':raise SystemExit(main())
