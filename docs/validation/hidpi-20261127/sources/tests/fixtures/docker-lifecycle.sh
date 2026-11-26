#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
set -eu
exec > /work/docker.log 2>&1
mount -t tmpfs -o mode=755 tmpfs /run
mkdir -p /dev/shm
mount -t tmpfs -o size=64m,mode=1777 tmpfs /dev/shm
test ! -e /etc/docker/daemon.json
! command -v crun
! printenv GOMAXPROCS
daemon=
cleanup() {
  if test -n "$daemon"; then kill "$daemon" 2>/dev/null || :; wait "$daemon" 2>/dev/null || :; fi
  cat /work/dockerd.log
}
trap cleanup EXIT
start_daemon() {
  /usr/sbin/dockerd > /work/dockerd.log 2>&1 & daemon=$!
  for i in $(seq 1 120); do
    if docker info > /work/docker-info.log 2>&1; then break; fi
    kill -0 "$daemon"; sleep 1
  done
  test "$(docker info --format '{{.DefaultRuntime}} {{.Driver}}')" = 'runc overlay2'
}
start_daemon
docker import /acceptance/container.tar trames-acceptance:local
docker volume create acceptance-data
docker network create acceptance-net
docker run -d --name acceptance-app --network acceptance-net -v acceptance-data:/data -p 18080:8080 trames-acceptance:local /bin/sh -ec '
 test "$$" = 1
 grep -Eq "^Seccomp:[[:space:]]+2$" /proc/self/status
 printf persistent >/data/value
 printf "http-payload\n" >/www/index.html
 /bin/httpd -f -p 8080 -h /www &
 server=$!
 trap "kill $server 2>/dev/null || :; wait $server || :; exit 0" TERM INT
 wait "$server"'
for i in $(seq 1 60); do
  if curl --fail --silent http://127.0.0.1:18080/ > /work/http.actual; then break; fi
  test "$(docker inspect --format '{{.State.Running}}' acceptance-app)" = true
  sleep 1
done
printf 'http-payload\n' > /work/http.expected
cmp /work/http.expected /work/http.actual
docker exec acceptance-app /bin/sh -ec 'test "$(cat /data/value)" = persistent; printf exec-ok' > /work/exec.actual
test "$(cat /work/exec.actual)" = exec-ok
docker run --rm --network acceptance-net trames-acceptance:local /bin/wget -qO- http://acceptance-app:8080/ > /work/dns.actual
cmp /work/http.expected /work/dns.actual
docker stop -t 2 acceptance-app
test "$(docker wait acceptance-app)" = 0
kill "$daemon"; wait "$daemon"; daemon=
start_daemon
test "$(docker inspect --format '{{.State.Running}}' acceptance-app)" = false
docker start acceptance-app
for i in $(seq 1 60); do
  if docker exec acceptance-app /bin/cat /data/value > /work/persisted.actual; then break; fi
  sleep 1
done
test "$(cat /work/persisted.actual)" = persistent
docker stop -t 2 acceptance-app
docker rm acceptance-app
docker volume rm acceptance-data
docker network rm acceptance-net
docker image rm trames-acceptance:local
printf 'docker: ok\n' > /work/docker.result
