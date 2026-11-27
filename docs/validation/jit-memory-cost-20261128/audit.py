#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Recheck frozen sampling and admission controls without perf, VM or workloads."""
import collections
import gzip
import hashlib
import json
from pathlib import Path
import re
import statistics

ROOT=Path(__file__).resolve().parent

def main():
    summary=json.loads((ROOT/'sample-summary.json').read_text())
    counts_by_mode={}
    for mode in ['worker','worker-private']:
        counts=collections.Counter()
        text=gzip.decompress((ROOT/f'jit-cost-perf-{mode}-full.txt.gz').read_bytes()).decode()
        assert '# Total Lost Samples: 0' in text
        for line in text.splitlines():
            match=re.match(r'\s*([0-9.]+)%\s+(\d+)\s+\[[^]]+\]\s+(.*?)\s+-\s+-\s*$',line)
            if match:counts[match[3]]+=int(match[2])
        expected=summary[mode]
        assert sum(counts.values())==expected['samples']
        assert counts=={x['symbol']:x['samples'] for x in expected['symbols']}
        for row in expected['symbols']:
            assert row['percent']==100*row['samples']/expected['samples']
        rows=[json.loads(line) for line in gzip.decompress((ROOT/f'jit-cost-profile-{mode}.jsonl.gz').read_bytes()).splitlines()]
        assert len(rows)==24
        for row in rows:check(row,'jit',mode)
        counts_by_mode[mode]=sum(counts.values())
    controls=json.loads((ROOT/'summary.json').read_text())
    for key,stored in controls.items():
        backend,mode=key.split(' / ')
        medians=[]
        for group in range(1,7):
            row=json.loads((ROOT/f'{group}-{backend}-{mode}.jsonl').read_text())
            check(row,backend,mode)
            medians.append(row['median_ns'])
        assert medians==stored['process_medians_ns']
        assert statistics.median(medians)==stored['median_ns']
    maps=json.loads(gzip.decompress((ROOT/'symbol-maps.json.gz').read_bytes()))
    assert len(maps)==48
    assert all('trames_jit_memory_leaf' in text and 'trames_jit_generated_block' in text for text in maps.values())
    receipt=json.loads((ROOT/'receipt.json').read_text())
    patch=gzip.decompress((ROOT/'diagnostic.patch.gz').read_bytes())
    assert hashlib.sha256(patch).hexdigest()==receipt['patch_sha256']
    print(json.dumps({'passed':True,'samples':counts_by_mode,'profile_processes':48,'unprofiled_controls':24,'symbol_maps':len(maps),'scope':'offline sample aggregation / actual admission / exact payload result / median / patch binding'},indent=2))

def check(row,backend,mode):
    assert row['backend']==backend and row['mode']==mode
    assert row['admission']=={'data_eligible':True,'exclusive':mode=='worker-private'}
    assert row['logical_guest_instructions']==5242881 and row['iterations']==1048576
    assert row['final_data']==6291456 and row['final_x0']==6291456 and row['final_pc']==0x1018
    assert len(row['samples'])==6
    for i,s in enumerate(row['samples']):
        assert s['x0']==(i+1)*1048576 and s['observed_instructions']==5242881
        assert s['warmup']==(i==0)
    assert sorted(s['ns'] for s in row['samples'][1:])[2]==row['median_ns']
    if backend=='jit':
        assert row['diagnostics']['fallback_instructions']==0 and row['diagnostics']['staged']==0

if __name__=='__main__':main()
