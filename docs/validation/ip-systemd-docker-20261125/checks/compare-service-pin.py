#!/usr/bin/env python3
"""Offline evidence comparison; never edits either product executable."""
import hashlib
import json
from pathlib import Path
import struct

OLD = b'76f860287368605e3697bd934816c18490967c2f'
receipt = json.loads(Path('/home/main.guest/trames-services-final-pin-build.json').read_text())
NEW = receipt['sources']['runtime']['commit'].encode('ascii')
paths = [Path('/home/main.guest/trames-services-qualified/runner'),
         Path('/home/main.guest/trames-services-final-pin-binary')]

def inspect(path):
    data = path.read_bytes()
    assert data[:6] == b'\x7fELF\x02\x01'
    start = struct.unpack_from('<Q', data, 40)[0]
    size, count, strings = struct.unpack_from('<HHH', data, 58)
    rows = [struct.unpack_from('<IIQQQQIIQQ', data, start + i * size)
            for i in range(count)]
    table = rows[strings]
    names = data[table[4]:table[4] + table[5]]
    sections = {names[r[0]:].split(b'\0', 1)[0].decode(): r for r in rows}
    note = sections['.note.gnu.build-id']
    offset = note[4]
    name_size, descriptor_size, kind = struct.unpack_from('<III', data, offset)
    assert kind == 3 and data[offset + 12:offset + 12 + name_size] == b'GNU\0'
    descriptor_start = offset + 12 + (name_size + 3) // 4 * 4
    normalized = bytearray(data)
    normalized[descriptor_start:descriptor_start + descriptor_size] = bytes(descriptor_size)
    # The non-loadable symbol/string tables differ in length after relinking.
    # e_shoff points to those inspection tables; no program segment uses it.
    normalized[40:48] = bytes(8)
    program_start = struct.unpack_from('<Q', data, 32)[0]
    program_size, program_count = struct.unpack_from('<HH', data, 54)
    loads = [struct.unpack_from('<IIQQQQQQ', data, program_start + i * program_size)
             for i in range(program_count)]
    return data, bytes(normalized), sections, [r for r in loads if r[0] == 1]

left, left_normal, left_sections, left_loads = inspect(paths[0])
right, right_normal, right_sections, right_loads = inspect(paths[1])
assert left.count(OLD) == 1 and right.count(NEW) == 1
right_normal = right_normal.replace(NEW, OLD)
assert left_loads == right_loads
for segment in left_loads:
    offset, size = segment[2], segment[5]
    assert left_normal[offset:offset + size] == right_normal[offset:offset + size], 'loadable differences exceed runtime pin and GNU build ID'
text = left_sections['.text']
assert text == right_sections['.text']
text_digest = hashlib.sha256(left[text[4]:text[4] + text[5]]).hexdigest()
result = dict(
    final_product_commit=receipt['sources']['product']['commit'],
    final_runtime_commit=NEW.decode('ascii'),
    old_binary_sha256=hashlib.sha256(left).hexdigest(),
    new_binary_sha256=hashlib.sha256(right).hexdigest(),
    old_binary_size=len(left),
    new_binary_size=len(right),
    executable_text_sha256=text_digest,
    all_PT_LOAD_headers_and_bytes_identical_after_normalizing_only=[
        'one embedded 40-byte runtime commit string', 'GNU build-ID note descriptor',
        'ELF e_shoff pointer to non-loadable section headers'],
    normalized_loadable_sha256=hashlib.sha256(b''.join(left_normal[s[2]:s[2] + s[5]] for s in left_loads)).hexdigest(),
    equal=True,
    note='Existing execution evidence retains its original binary/build provenance; no acceptance fingerprints are rewritten.',
)
Path('/mnt/trames-output/ip-systemd-docker-20261125/pin-equivalence.json').write_text(
    json.dumps(result, indent=2) + '\n')
print(json.dumps(result, indent=2))
