#!/usr/bin/env python3
"""ALSA client reference vs KVM, using software PCM devices (no soundcard claim)."""
from pathlib import Path
import json,os,shutil,subprocess
root=Path(__file__).resolve().parents[1];os.chdir(root)
binary=Path(os.environ.get('TRAMES_BINARY','target/release/trames')).resolve()
out=Path(os.environ.get('TRAMES_VALIDATION_OUTPUT','target/validation/audio')).resolve();out.mkdir(parents=True,exist_ok=True)
client=out/'audio';plugin=out/'libasound_module_pcm_trames.so';clock=out/'libasound_module_pcm_trames_clock.so'
def run(args,**kw): return subprocess.run([str(a) for a in args],check=True,**kw)
run(['cc','-O2',root/'tests/fixtures/audio.c','-lasound','-o',client])
run(['cc','-O2','-DPIC','-shared','-fPIC',root/'runtime/crates/trames-audio/guest/alsa.c',root/'runtime/crates/trames-audio/guest/calls.S','-lasound','-o',plugin])
run(['cc','-O2','-DPIC','-shared','-fPIC',root/'runtime/crates/trames-audio/tests/clock-pcm.c','-lasound','-o',clock])
rootfs=out/'rootfs';(rootfs/'test').mkdir(parents=True,exist_ok=True)
for file in [client,plugin]:shutil.copy2(file,rootfs/'test'/file.name)
for file in [client,plugin]:
    for line in subprocess.check_output(['ldd',file],text=True).splitlines():
        for word in line.split():
            if word.startswith('/'):
                source=Path(word);dest=rootfs/source.relative_to('/');dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(source,dest)
# Guest config intentionally uses only the built-in terminal PCM, without a
# distribution-wide plugin scan. Native host config is separate and retained.
guest_config=rootfs/'test/asound.conf';guest_config.write_text('pcm.trames {type trames}\n')
host_config=out/'host.conf';samples=out/'samples.raw'
host_config.write_text(f'</usr/share/alsa/alsa.conf>\npcm.trames_clock {{type trames_clock}}\npcm.trames_file {{type file slave.pcm null file "{samples}" format raw}}\n')
env=os.environ|{'ALSA_CONFIG_PATH':str(host_config),'ALSA_PLUGIN_DIR':str(out),'TRAMES_AUDIO_OUTPUT_DEVICE':'trames_file','TRAMES_AUDIO_INPUT_DEVICE':'trames_clock'}
results=[]
for mode,device,flag in [('playback','trames_file','--audio'),('capture','trames_clock','--audio-input')]:
    native=subprocess.run([client,device,mode],env=env,capture_output=True,timeout=10)
    assert native.returncode==0,(mode,native.stderr)
    reference=samples.read_bytes() if mode=='playback' else None
    for transport in ['smccc','mmio']:
        guest=subprocess.run([binary,'run',flag,'--rootfs',rootfs,'--timeout-ms','5000','--setenv','ALSA_CONFIG_PATH=/test/asound.conf','--setenv','ALSA_PLUGIN_DIR=/test','--','/test/audio','trames',mode],env=env|{'TRAMES_KVM_TRANSPORT':transport},capture_output=True,timeout=10)
        passed=guest.returncode==0 and guest.stdout==native.stdout and (reference is None or samples.read_bytes()==reference)
        results.append(dict(case=f'{mode}/{transport}',passed=passed,code=guest.returncode,stdout=guest.stdout.decode(errors='replace'),stderr=guest.stderr.decode(errors='replace')))
        print(results[-1],flush=True)
# Capability denial is distinct from default host library/device lookup.
denied=subprocess.run([binary,'run','--rootfs',rootfs,'--setenv','ALSA_CONFIG_PATH=/test/asound.conf','--setenv','ALSA_PLUGIN_DIR=/test','--','/test/audio','trames','playback'],env=env,capture_output=True,timeout=10)
results.append(dict(case='disabled-by-default',passed=denied.returncode!=0,code=denied.returncode))
(out/'results.json').write_text(json.dumps(results,indent=2)+'\n')
assert all(r['passed'] for r in results),results
