from pathlib import Path
import copy,hashlib,importlib.util,json,sys
here=Path(__file__).resolve().parent
sys.path.insert(0,str(here/'audit-support'))
source=here/'audit-controller.py'
spec=importlib.util.spec_from_file_location('trames_benchmark_audit',source)
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
out=Path(sys.argv[1]);samples=out/'samples.json';original=samples.read_bytes();rows=json.loads(original)
chosen=next(row for row in rows if row['phase']=='sample' and row['lane']=='smccc')
folder=out/'cases'/chosen['id'];result=folder/'result.json';stdout=folder/'stdout'
saved_result=result.read_bytes();saved_stdout=stdout.read_bytes();checks=[]
def rejected(name, action):
 try:
  action();module.audit(out)
 except ValueError as error:
  checks.append(dict(name=name,passed=True,reason=str(error)))
 else:raise AssertionError('auditor accepted '+name)
 finally:
  samples.write_bytes(original);result.write_bytes(saved_result);stdout.write_bytes(saved_stdout)
rejected('changed raw output',lambda:stdout.write_bytes(saved_stdout+b'corruption'))
rejected('missing sample',lambda:samples.write_text(json.dumps(rows[:-1])))
def changed_work():
 altered=copy.deepcopy(rows);row=next(r for r in altered if r['id']==chosen['id']);row['iterations']+=1
 result.write_text(json.dumps(row));samples.write_text(json.dumps(altered))
rejected('self-consistent metadata differs from plan',changed_work)
altered=copy.deepcopy(rows);row=next(r for r in altered if r['id']==chosen['id'])
row['work']['checksum']+=1;stdout.write_text(json.dumps(row['work'])+'\n');row['logs']['stdout']=module.digest(stdout)
result.write_text(json.dumps(row));samples.write_text(json.dumps(altered))
try:
 assert module.audit(out)==1
 checks.append(dict(name='self-consistent wrong workload result differs from native',passed=True))
finally:
 samples.write_bytes(original);result.write_bytes(saved_result);stdout.write_bytes(saved_stdout)
assert module.audit(out)==0
checks.append(dict(name='restored original evidence accepted',passed=True))
record=dict(passed=True,cases=checks,controller_sha256=module.digest(source),inputs_sha256=module.digest(out/'inputs.json'),test_sha256=module.digest(Path(__file__)))
(out/'audit-negative-tests.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record,indent=2))
