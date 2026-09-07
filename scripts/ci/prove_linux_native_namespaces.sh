#!/usr/bin/env bash
# Fail before compilation if the environment cannot exercise the actual sandbox.
set -euo pipefail
test "$(id -u)" = 10001
test "$(id -g)" = 10001
for native_interface in /sys/class/net/*; do
  test "${native_interface##*/}" = lo
done
native_status="$(< /proc/$$/status)"
for field in CapInh CapPrm CapEff CapBnd CapAmb; do
  grep -Eq "^${field}:[[:space:]]+0+$" <<< "${native_status}"
done
grep -Eq '^NoNewPrivs:[[:space:]]+1$' <<< "${native_status}"
# Exclude inheritance as the reason for the renderer's later nonzero filter count.
# A count difference alone does not identify a filter's installer or policy.
grep -Eq '^Seccomp:[[:space:]]+0$' <<< "${native_status}"
grep -Eq '^Seccomp_filters:[[:space:]]+0$' <<< "${native_status}"
for field in user mnt pid; do
  export "ZEPHIUM_PARENT_NS_${field}=$(readlink "/proc/$$/ns/${field}")"
done
timeout --signal=TERM --kill-after=2s 10s \
  bwrap --unshare-user --unshare-pid --unshare-net \
    --ro-bind / / --proc /proc --dev /dev --new-session --die-with-parent \
    bash -euo pipefail -c '
      for field in user mnt pid; do
        key="ZEPHIUM_PARENT_NS_${field}"
        test "$(readlink "/proc/$$/ns/${field}")" != "${!key}"
      done
    '
echo 'native environment: non-root; capabilities=none; no-new-privileges=true; inherited-seccomp=none; distinct-user-mount-pid=true'
