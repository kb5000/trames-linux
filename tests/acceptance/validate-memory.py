#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Original 4KiB-aligned Linux ELF: native vs real KVM, with bounded artifacts."""
import argparse
import json
import os
from pathlib import Path
import platform
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'runtime/tests/acceptance'))
from common import digest, save, tree_digest
from run import execute, host, receipt

MODES = ('geometry', 'protection', 'holes', 'owners', 'file-offsets', 'truncate',
         'discard-remap', 'cow', 'threads', 'retire', 'exec')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--runner', type=Path)
    p.add_argument('--receipt', type=Path)
    p.add_argument('--native-only', action='store_true')
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    if platform.system() != 'Linux' or os.sysconf('SC_PAGE_SIZE') != 4096:
        p.error('requires actual Linux 4096-byte host pages')
    if not a.native_only and not (a.runner and a.receipt):
        p.error('KVM mode requires --runner and --receipt')
    out = a.output.resolve(); out.mkdir(parents=True, exist_ok=False)
    runner = a.runner.resolve() if a.runner else None
    build = receipt(a.receipt.resolve(), runner) if not a.native_only else None
    if runner:
        doctor = subprocess.run([runner, 'doctor', '--json', '--require-kvm'], capture_output=True, timeout=15)
        (out / 'doctor.stdout').write_bytes(doctor.stdout); (out / 'doctor.stderr').write_bytes(doctor.stderr)
        if doctor.returncode:
            raise RuntimeError('real 4KiB KVM preflight failed')
    source = ROOT / 'tests/fixtures/memory-4k.c'; binary = out / 'original-memory-elf'
    command = [os.environ.get('CC', 'cc'), '-O2', '-static', '-pthread', '-Wall', '-Wextra', '-Werror',
               '-Wl,-z,max-page-size=4096', '-Wl,-z,common-page-size=4096', str(source), '-o', str(binary)]
    with (out / 'fixture.build.log').open('wb') as log:
        subprocess.run(command, check=True, stdout=log, stderr=subprocess.STDOUT)
    headers = subprocess.check_output(['readelf', '--wide', '-l', binary], text=True)
    (out / 'elf-segments.txt').write_text(headers)
    segments = [line.split() for line in headers.splitlines() if line.strip().startswith('LOAD ')]
    if not segments or any(int(row[-1], 16) != 4096 for row in segments):
        raise RuntimeError('fixture does not have strictly 4KiB-aligned LOAD segments')
    # A static ELF is imported directly; no rootfs image or repeated library copies.
    save(out / 'inputs.json', dict(build=build, controller_sha256=digest(Path(__file__)),
         fixture_source_sha256=digest(source), fixture_binary_sha256=digest(binary),
         compiler_command=command, compiler_version=subprocess.check_output([command[0], '--version'], text=True),
         load_alignments=[int(row[-1], 16) for row in segments], host=host(),
         scope='core-memory-4k', full_product_4k_qualified=False))
    rows = []; native = {}
    for mode in MODES:
        folder = out / ('native-' + mode); folder.mkdir()
        result = execute([binary, mode], os.environ.copy(), folder, 40)
        passed = result['status'] == 'completed' and result['exit_code'] == 0 and (folder / 'stdout').read_bytes() == (mode + ': ok\n').encode() and not (folder / 'stderr').read_bytes()
        native[mode] = passed
        rows.append(dict(id=folder.name, **result) | dict(status='passed' if passed else 'failed'))
        print(rows[-1], flush=True)
    for transport in ([] if a.native_only else ['smccc', 'mmio']):
        for workers in [1, 2, 4]:
            for mode in MODES:
                folder = out / f'{mode}-{transport}-{workers}'; folder.mkdir()
                if not native[mode]:
                    rows.append(dict(id=folder.name, status='reference_failed')); continue
                # Foreground ELF import names argv[0] /program; exec() uses that original path.
                result = execute([runner, 'run', '--cpus', str(workers), '--timeout-ms', '40000', '--', binary, mode],
                                 os.environ | {'TRAMES_KVM_TRANSPORT': transport}, folder, 45)
                passed = result['status'] == 'completed' and result['exit_code'] == 0 and (folder / 'stdout').read_bytes() == (out / ('native-' + mode) / 'stdout').read_bytes() and not (folder / 'stderr').read_bytes()
                rows.append(dict(id=folder.name, **result) | dict(status='passed' if passed else result['status'] if result['status'] != 'completed' else 'failed'))
                print(rows[-1], flush=True)
                save(out / 'results.json', dict(cases=rows, passed=sum(r['status'] == 'passed' for r in rows), planned=len(MODES) * 7, full_product_4k_qualified=False))
    planned = len(MODES) * (1 if a.native_only else 7)
    result = dict(cases=rows, passed=sum(r['status'] == 'passed' for r in rows), planned=planned,
                  native_only=a.native_only, full_product_4k_qualified=False,
                  remaining_release_domains=['same-host KVM plus accelerated GPU', 'image/FD/sync imports', 'physical PCM and A/V clocks', 'hardware video'])
    save(out / 'results.json', result)
    return 0 if result['passed'] == planned else 1


if __name__ == '__main__':
    raise SystemExit(main())
