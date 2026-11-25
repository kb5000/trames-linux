#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Freeze a native-architecture Debian rootfs with real IP/systemd/Docker tools.

Run once with sudo on Linux. Package installation occurs only in an owned
chroot in disposable mount/PID namespaces. Execution acceptance is offline.
"""
import argparse, hashlib, json, os
from pathlib import Path
import platform, shutil, subprocess, sys, tarfile
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'runtime/tests/acceptance'))
from common import digest, save, tree_digest
PACKAGES=['systemd-sysv','dbus','iproute2','iputils-ping','python3','procps','util-linux','docker.io','docker-cli','iptables','busybox-static','curl','ca-certificates']


def install(root):
    for name in ('null','zero','full','random','urandom','tty'):
        dest=root/'dev'/name;dest.parent.mkdir(parents=True,exist_ok=True)
        if dest.is_symlink():dest.unlink()
        dest.touch(exist_ok=True)
        subprocess.run(['mount','--bind','/dev/'+name,str(dest)],check=True)
    subprocess.run(['mount','-t','sysfs','-o','ro,nosuid,nodev,noexec','sysfs',str(root/'sys')],check=True)
    policy=root/'usr/sbin/policy-rc.d'
    old=policy.read_bytes() if policy.exists() else None
    mode=policy.stat().st_mode&0o777 if old is not None else None
    resolver=root/'etc/resolv.conf'; saved_link=os.readlink(resolver) if resolver.is_symlink() else None
    saved_resolver=resolver.read_bytes() if resolver.exists() else None
    resolver.unlink(missing_ok=True);resolver.write_bytes(Path('/etc/resolv.conf').read_bytes())
    policy.write_text('#!/bin/sh\nexit 101\n');policy.chmod(0o755)
    env=dict(os.environ,DEBIAN_FRONTEND='noninteractive',container='trames-acceptance')
    try:
        for args in [['apt-get','update'],['apt-get','install','-y','--no-install-recommends',*PACKAGES],['apt-get','clean']]:
            subprocess.run(['chroot',str(root),*args],env=env,check=True)
    finally:
        policy.unlink(missing_ok=True)
        if old is not None:policy.write_bytes(old);policy.chmod(mode)
        resolver.unlink(missing_ok=True)
        if saved_link is not None:resolver.symlink_to(saved_link)
        elif saved_resolver is not None:resolver.write_bytes(saved_resolver)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source',type=Path);p.add_argument('--sha256');p.add_argument('--output',type=Path)
    p.add_argument('--install',type=Path,help=argparse.SUPPRESS)
    a=p.parse_args()
    if platform.system()!='Linux' or os.geteuid()!=0:p.error('run with sudo on native Linux')
    if a.install:return install(a.install.resolve())
    if not a.source or not a.output or not a.sha256:p.error('--source, --sha256 and --output required')
    if digest(a.source)!=a.sha256:p.error('source image SHA256 mismatch')
    a.output=a.output.resolve();a.output.mkdir(parents=True,exist_ok=False)
    root=a.output/'rootfs';root.mkdir()
    subprocess.run(['tar','--numeric-owner','-xpf',str(a.source.resolve()),'-C',str(root)],check=True)
    for name in ('proc','sys','dev','work','acceptance','run','tmp'):(root/name).mkdir(exist_ok=True)
    (root/'tmp').chmod(0o1777)
    subprocess.run(['unshare','--mount','--pid','--fork','--kill-child=SIGKILL','--mount-proc='+str(root/'proc'),sys.executable,str(Path(__file__).resolve()),'--install',str(root)],check=True)
    # Installation-only bind targets are ordinary placeholders after unshare
    # exits. Leave /dev to the runtime/nspawn rather than import fake devices.
    for name in ('null','zero','full','random','urandom','tty'):
        (root/'dev'/name).unlink(missing_ok=True)
    versions=subprocess.check_output(['chroot',str(root),'dpkg-query','-W','-f=${binary:Package}\t${Version}\n'],text=True).splitlines()
    # Stable image-local aliases; stock distribution binaries remain unchanged.
    for name in ('ip','bridge'):
        target=root/'usr/bin'/name
        if not target.exists():target.symlink_to('/usr/sbin/'+name)
    subprocess.run(['cc','-O2','-static','-Wall','-Wextra','-Werror',str(ROOT/'tests/fixtures/systemd-boot.c'),'-o',str(root/'acceptance/systemd-boot')],check=True)
    shutil.copy2(ROOT/'tests/fixtures/docker-lifecycle.sh',root/'acceptance/docker-lifecycle.sh')
    # One small original distribution image, imported by Docker offline.
    payload=a.output/'payload';(payload/'bin').mkdir(parents=True)
    shutil.copy2(root/'bin/busybox',payload/'bin/busybox')
    for name in ('sh','sleep','cat','mkdir','httpd','wget','ping','ip','grep','stat'):(payload/'bin'/name).symlink_to('busybox')
    for name in ('tmp','proc','dev','sys','data','www','etc'):(payload/name).mkdir()
    with tarfile.open(root/'acceptance/container.tar','w') as archive:archive.add(payload,arcname='.')
    shutil.rmtree(payload)
    save(a.output/'bundle.json',dict(schema_version=1,machine=platform.machine(),host_kernel=platform.release(),rootfs_sha256=tree_digest(root),source_sha256=a.sha256,packages=versions,tools={}))
    print(a.output,flush=True)


if __name__=='__main__':main()
