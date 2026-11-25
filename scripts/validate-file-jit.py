#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Real guest file permissions, original generated code, and store reopen."""
from pathlib import Path
import json, os, shutil, subprocess, tempfile
ROOT=Path(__file__).resolve().parents[1]
binary=Path(os.environ.get('TRAMES_BINARY',ROOT/'target/release/trames')).resolve()
out=Path(os.environ.get('TRAMES_VALIDATION_OUTPUT',ROOT/'target/validation/file-jit')).resolve()
out.mkdir(parents=True,exist_ok=False)
fixture=out/'file-jit'
subprocess.run(['cc','-O2','-static','-Wall','-Wextra','-Werror',str(ROOT/'tests/fixtures/file-jit.c'),'-o',str(fixture)],check=True)
rows=[]
for transport in ('smccc','mmio'):
 for cpus in (1,4):
  with tempfile.TemporaryDirectory(prefix='trames-file-jit-') as tmp:
   root=Path(tmp)/'rootfs';(root/'work').mkdir(parents=True);shutil.copy2(fixture,root/'work/file-jit')
   store=Path(tmp)/'store'
   for filesystem in ('host','store'):
    for mode in ('file','jit'):
     command=[str(binary),'run','--cpus',str(cpus),'--timeout-ms','30000']
     if filesystem=='store':command+=['--store',str(store)]
     if filesystem=='host' or mode=='file':command+=['--rootfs',str(root)]
     command+=['--','/work/file-jit',mode]
     p=subprocess.run(command,env=dict(os.environ,TRAMES_KVM_TRANSPORT=transport),capture_output=True,timeout=35)
     label=f'{transport}-{cpus}-{filesystem}-{mode}';(out/(label+'.stdout')).write_bytes(p.stdout);(out/(label+'.stderr')).write_bytes(p.stderr)
     row=dict(id=label,passed=p.returncode==0 and p.stdout==(b'file-cache: ok\n' if mode=='file' else b'jit-cache: ok\n') and not p.stderr,exit_code=p.returncode)
     rows.append(row);print(label,row['passed'],flush=True)
(out/'results.json').write_text(json.dumps(rows,indent=2)+'\n')
if not all(row['passed'] for row in rows):raise SystemExit(1)
