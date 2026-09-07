#!/usr/bin/env bash
# Run the unchanged native gates in a cap-free Fedora userspace on the hosted VM.
set -euo pipefail

[[ "${GITHUB_RUN_ID:?}" =~ ^[0-9]+$ ]]
[[ "${GITHUB_RUN_ATTEMPT:?}" =~ ^[0-9]+$ ]]
native_name="zephium-native-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}"
native_volume="${native_name}-home"

# Every mode, including cleanup, is host-policy-sensitive. Never run it against
# a PR or a caller-selected checkout, even if invoked outside the job gate.
if [[ "${GITHUB_EVENT_NAME:?}" != push && "${GITHUB_EVENT_NAME}" != workflow_dispatch ]] \
  || [[ "${GITHUB_REF:?}" != refs/heads/main ]] \
  || [[ ! "${GITHUB_SHA:?}" =~ ^[0-9a-f]{40}$ ]]; then
  echo 'native host setup requires a trusted main event' >&2
  exit 1
fi
case "${GITHUB_EVENT_NAME}" in
  push)
    native_input_valid=false
    if [[ -z "${NATIVE_CHECKOUT_REF:-}" ]]; then native_input_valid=true; fi
    ;;
  workflow_dispatch)
    native_input_valid=false
    if [[ "${NATIVE_CHECKOUT_REF:-}" =~ ^[0-9a-f]{40}$ ]] && [[ "${NATIVE_CHECKOUT_REF}" = "${GITHUB_SHA}" ]]; then
      native_input_valid=true
    fi
    ;;
esac
if [[ "${native_input_valid}" != true ]]; then
  echo 'native host setup requires an event-bound checkout input' >&2
  exit 1
fi
if [[ "$(git rev-parse HEAD)" != "${GITHUB_SHA}" ]]; then
  echo 'native host setup requires the exact event checkout' >&2
  exit 1
fi

case "${1:-}" in
  start)
    [[ "$#" == 1 ]]
    test "$(uname -s)" = Linux
    test -f "${GITHUB_WORKSPACE:?}/scripts/ci/linux-native.Dockerfile"
    command -v apparmor_parser >/dev/null
    native_rustup="$(rustup show home)"
    native_rust_bin="$(dirname "$(command -v rustup)")"
    test -d "${native_rustup}/toolchains/1.95.0-x86_64-unknown-linux-gnu"
    # Only checked-in CI build definitions form the context; no workspace mount,
    # credentials or host sockets are passed to this package build.
    docker build --file scripts/ci/linux-native.Dockerfile --tag "${native_name}" scripts/ci
    sudo apparmor_parser --replace scripts/ci/linux-native.apparmor
    docker volume create "${native_volume}" >/dev/null
    # systempaths=unconfined is a containment tradeoff needed only for procfs.
    # Restore the non-proc masks with empty read-only tmpfs mounts. Powercap
    # is kernel-dependent; never try creating a missing directory in sysfs.
    test -d /sys/firmware
    native_system_masks=(--tmpfs /sys/firmware:ro,nosuid,nodev,noexec,size=1m,mode=000)
    if test -d /sys/devices/virtual/powercap; then
      native_system_masks+=(--tmpfs /sys/devices/virtual/powercap:ro,nosuid,nodev,noexec,size=1m,mode=000)
    fi
    docker run --detach --init --name "${native_name}" \
      --user 10001:10001 --cap-drop ALL --security-opt no-new-privileges \
      --security-opt seccomp=unconfined --security-opt apparmor=zephium-native-ci \
      --security-opt systempaths=unconfined \
      --read-only --pids-limit 2048 --shm-size 256m \
      --tmpfs /tmp:rw,nosuid,nodev,size=1g \
      "${native_system_masks[@]}" \
      --mount "type=bind,source=${GITHUB_WORKSPACE},target=/workspace,readonly" \
      --mount "type=bind,source=${native_rustup},target=/opt/rustup,readonly" \
      --mount "type=bind,source=${native_rust_bin},target=/opt/rust-bin,readonly" \
      --mount "type=volume,source=${native_volume},target=/home/native" \
      --env "ZEPHIUM_MIN_WEBKITGTK_VERSION=${ZEPHIUM_MIN_WEBKITGTK_VERSION:?}" \
      "${native_name}" >/dev/null
    ;;
  diagnose)
    [[ "$#" == 1 ]]
    # Only this trusted event's container metadata and a bounded kernel-log
    # tail are read. Never print raw audit records, arbitrary profiles or paths.
    test "$(docker inspect --format '{{.AppArmorProfile}}' "${native_name}")" = zephium-native-ci
    test "$(docker inspect --format '{{len .HostConfig.MaskedPaths}} {{len .HostConfig.ReadonlyPaths}}' "${native_name}")" = '0 0'
    sudo grep -Fx 'zephium-native-ci (unconfined)' /sys/kernel/security/apparmor/profiles
    native_started="$(docker inspect --format '{{.State.StartedAt}}' "${native_name}")"
    native_since="$(date --date="${native_started}" +%s)"
    [[ "${native_since}" =~ ^[0-9]+$ ]]
    timeout --signal=TERM --kill-after=2s 10s \
      sudo journalctl --kernel --since="@${native_since}" --lines=256 --no-pager --output=cat \
      | awk -f scripts/ci/summarize_linux_native_denials.awk
    ;;
  seal)
    [[ "$#" == 1 ]]
    docker network disconnect bridge "${native_name}"
    test "$(docker inspect --format '{{len .NetworkSettings.Networks}}' "${native_name}")" = 0
    ;;
  exec)
    [[ "$#" == 2 ]]
    test -f "$2"
    # The GitHub-generated script arrives on stdin. Runner temp, environment
    # files, credentials, Docker socket and host process namespace are not mounted.
    native_offline=false
    if [[ "$(docker inspect --format '{{len .NetworkSettings.Networks}}' "${native_name}")" == 0 ]]; then
      native_offline=true
    fi
    docker exec --interactive --user 10001:10001 --env "CARGO_NET_OFFLINE=${native_offline}" "${native_name}" \
      bash --noprofile --norc -e -o pipefail -s < "$2"
    ;;
  stop)
    [[ "$#" == 1 ]]
    if docker container inspect "${native_name}" >/dev/null 2>&1; then
      docker container rm --force "${native_name}" >/dev/null
    fi
    if docker volume inspect "${native_volume}" >/dev/null 2>&1; then
      docker volume rm "${native_volume}" >/dev/null
    fi
    if docker image inspect "${native_name}" >/dev/null 2>&1; then
      docker image rm "${native_name}" >/dev/null
    fi
    if sudo test -r /sys/kernel/security/apparmor/profiles \
      && sudo grep -q '^zephium-native-ci ' /sys/kernel/security/apparmor/profiles; then
      sudo apparmor_parser --remove scripts/ci/linux-native.apparmor
    fi
    ;;
  *) echo 'expected start, seal, exec SCRIPT, diagnose, or stop' >&2; exit 2 ;;
esac
