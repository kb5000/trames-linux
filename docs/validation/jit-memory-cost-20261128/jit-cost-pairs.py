import os,json,subprocess,statistics
from pathlib import Path
root=Path('/mnt/trames-output/jit-cost-pairs');root.mkdir(exist_ok=True)
cases=[('jit','worker'),('jit','worker-private'),('interpreter','worker-private'),('interpreter','worker')]
rows={}
for group in range(1,7):
 for backend,mode in (cases if group%2 else list(reversed(cases))):
  p=root/f'{group}-{backend}-{mode}.jsonl'
  result=subprocess.run(['taskset','-c','2','/mnt/trames-output/linux-desktop-target/release/trames-cpu-backends',backend,mode],capture_output=True,timeout=60)
  p.write_bytes(result.stdout);assert result.returncode==0,result.stderr
  row=json.loads(result.stdout);assert row['admission']=={'data_eligible':True,'exclusive':mode=='worker-private'}
  assert row['final_data']==6291456 and row['final_x0']==6291456
  assert row['diagnostics'].get('fallback_instructions',0)==0
  rows.setdefault(backend+' / '+mode,[]).append(row['median_ns'])
summary={key:{'process_medians_ns':values,'median_ns':statistics.median(values)} for key,values in rows.items()}
(root/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps(summary,indent=2))
