# SPDX-License-Identifier: GPL-3.0-or-later
from pathlib import Path
import hashlib,json,os,subprocess,tempfile,shutil
script=Path('/mnt/trames-source/scripts/build-busybox.sh');prefix=script.read_text().split('fetch musl-1.2.5.tar.gz ',1)[0]
root=Path(tempfile.mkdtemp(prefix='trames-download-tests-'));(root/'bin').mkdir();(root/'scripts').mkdir()
try:
 stub=root/'bin/curl';stub.write_text('''#!/usr/bin/env python3
import pathlib,sys
args=sys.argv[1:];target=pathlib.Path(args[args.index('--output')+1]);origin=args[args.index('--output')-1]
if origin=='fail':target.write_bytes(b'partial');raise SystemExit(28)
target.write_bytes(b'valid-archive' if origin=='good' else b'tampered')
''');stub.chmod(0o755)
 digest=hashlib.sha256(b'valid-archive').hexdigest();cases=[]
 for name,origins,existing,expected in [('fallback',['fail','good'],None,True),('tampered-download',['bad','good'],None,False),('tampered-cache',['good'],b'bad-cache',False),('valid-cache',['fail'],b'valid-archive',True)]:
  cache=root/name;cache.mkdir()
  if existing is not None:(cache/'fixture.tar').write_bytes(existing)
  command=prefix+'\nfetch fixture.tar '+origins[0]+' '+digest+' '+' '.join(origins[1:])
  p=subprocess.run(['bash','-c',command,str(root/'scripts/build-busybox.sh')],env=os.environ|{'TRAMES_SOURCE_CACHE':str(cache),'PATH':str(root/'bin')+':'+os.environ['PATH']},capture_output=True,text=True)
  assert (p.returncode==0)==expected,(name,p.stdout,p.stderr)
  assert not list(cache.glob('.*')),name
  if expected:assert (cache/'fixture.tar').read_bytes()==b'valid-archive'
  elif existing is None:assert not (cache/'fixture.tar').exists()
  else:assert (cache/'fixture.tar').read_bytes()==existing
  cases.append(dict(id=name,exit_code=p.returncode,passed=True,stdout=p.stdout,stderr=p.stderr))
 result=dict(source_sha256=hashlib.sha256(script.read_bytes()).hexdigest(),cases=cases,passed=4,planned=4)
 Path('/mnt/trames-output/kvm-budget-download-check.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
finally:shutil.rmtree(root)
