from pathlib import Path
import hashlib,json,os,selectors,signal,subprocess,sys,time
root=Path(sys.argv[1] if len(sys.argv)>1 else '/home/trames/gpu-acceptance')
env=dict(os.environ,XDG_RUNTIME_DIR='/run/user/1000',WAYLAND_DISPLAY='wayland-0',DISPLAY=':0',VK_DRIVER_FILES='/usr/share/vulkan/icd.d/virtio_icd.json')
rows=[]
for name,command in [('vulkan-headless',[root/'native','headless']),('vulkan-wayland',['vkcube-wayland','--c','60'])]:
    result=subprocess.run(command,env=env,capture_output=True,timeout=20)
    (root/(name+'.stdout')).write_bytes(result.stdout);(root/(name+'.stderr')).write_bytes(result.stderr)
    passed=result.returncode==0 and (result.stdout==b'headless: ok\n' if name=='vulkan-headless' else b'Virtio-GPU Venus (' in result.stdout+result.stderr)
    rows.append(dict(id=name,exit_code=result.returncode,passed=passed));print(rows[-1],flush=True)
name='vulkan-xwayland-pixels';passed=False
with (root/(name+'.stdout')).open('wb') as out,(root/(name+'.stderr')).open('wb') as err:
    process=subprocess.Popen([root/'native'],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=err,start_new_session=True)
    try:
        data=b'';deadline=time.monotonic()+20
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout,selectors.EVENT_READ)
            while b'wsi-ready frames=16\n' not in data:
                remaining=deadline-time.monotonic()
                if remaining<=0 or not selector.select(remaining):raise TimeoutError('frame not ready')
                chunk=os.read(process.stdout.fileno(),65536)
                if not chunk:raise RuntimeError('renderer exited: '+str(process.wait()))
                data+=chunk;out.write(chunk);out.flush()
        for attempt in range(20):
            pixels=subprocess.run([root/'probe','Trames WSI native'],env=env,capture_output=True,timeout=3)
            (root/'pixels.stdout').write_bytes(pixels.stdout);(root/'pixels.stderr').write_bytes(pixels.stderr)
            if pixels.returncode==0:break
            time.sleep(.1)
        assert pixels.returncode==0 and pixels.stdout==b'pixels: 320x240 green\n'
        process.stdin.write(b'q');process.stdin.flush()
        rest,_=process.communicate(timeout=10);out.write(rest)
        assert process.returncode==0
        passed=True
    except Exception as error:
        err.write((repr(error)+'\n').encode())
    finally:
        if process.poll() is None:
            os.killpg(process.pid,signal.SIGTERM);process.wait(timeout=5)
rows.append(dict(id=name,passed=passed));print(rows[-1],flush=True)
record=dict(cases=rows,passed=sum(row['passed'] for row in rows),planned=3,icd=env['VK_DRIVER_FILES'],host_page_bytes=os.sysconf('SC_PAGE_SIZE'),nested_kvm_available=Path('/dev/kvm').exists(),fixture_hashes={name:hashlib.sha256((root/name).read_bytes()).hexdigest() for name in ['native','probe']})
(root/'results.json').write_text(json.dumps(record,indent=2)+'\n')
assert record['passed']==3
