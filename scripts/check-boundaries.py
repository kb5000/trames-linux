#!/usr/bin/env python3
"""Validate the real shared-runtime pin and product execution boundaries."""
from pathlib import Path
import json
import subprocess

root = Path(__file__).resolve().parents[1]
pin = json.loads((root / 'runtime-version.json').read_text())
expected = 'https://github.com/kb5000/trames-runtime'
assert pin['repository'] == expected
module_url = subprocess.check_output(['git', 'config', '-f', '.gitmodules', 'submodule.runtime.url'], cwd=root, text=True).strip()
assert module_url.removesuffix('.git') == expected
actual = subprocess.check_output(['git', '-C', str(root / 'runtime'), 'rev-parse', 'HEAD'], text=True).strip()
assert actual == pin['commit'], (actual, pin['commit'])
assert not subprocess.check_output(['git', '-C', str(root / 'runtime'), 'status', '--porcelain'], text=True).strip(), 'runtime checkout must be clean'
common = (root / 'crates/trames-kvm/src/linux.rs').read_text()
assert 'KVM_REG_ARM_CORE' not in common
ram = (root / 'crates/trames-kvm/src/storage.rs').read_text()
assert 'KVM_ARM_VM_SMCCC_CTRL' not in ram and 'KVM_REG_ARM_CORE' not in ram
assert (root / 'crates/trames-kvm/src/aarch64/vectors.S').is_file()
print(f'boundaries OK; runtime {actual}')
