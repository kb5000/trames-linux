from pathlib import Path
import os,subprocess
service='''[Unit]
Description=Trames GPU test display
After=systemd-user-sessions.service seatd.service
Requires=seatd.service
Conflicts=getty@tty1.service

[Service]
User=trames
Environment=XDG_SESSION_TYPE=wayland
Environment=XDG_RUNTIME_DIR=/run/user/1000
Environment=LIBSEAT_BACKEND=seatd
ExecStart=/usr/bin/dbus-run-session -- /usr/bin/weston --backend=drm --renderer=gl --socket=wayland-0 --xwayland --log=/home/trames/weston.log
Restart=on-failure
RestartSec=3

[Install]
WantedBy=multi-user.target
'''
Path('/etc/systemd/system/trames-gpu-display.service').write_text(service)
directory=Path('/home/trames/.config');directory.mkdir(exist_ok=True)
config=directory/'weston.ini'
config.write_text('[core]\nidle-time=0\nxwayland=true\n[shell]\nbackground-color=0xff1e293b\nlocking=false\n')
os.chown(directory,1000,1000);os.chown(config,1000,1000)
subprocess.run(['systemctl','daemon-reload'],check=True)
subprocess.run(['systemctl','enable','--now','seatd'],check=True)
subprocess.run(['loginctl','enable-linger','trames'],check=True)
subprocess.run(['systemctl','enable','--now','trames-gpu-display.service'],check=True)
