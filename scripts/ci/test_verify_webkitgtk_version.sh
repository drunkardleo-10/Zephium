#!/usr/bin/env bash

set -euo pipefail
export LC_ALL=C

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
helper="${root}/scripts/ci/verify_webkitgtk_version.sh"
temporary="$(mktemp -d)"
trap 'rm -rf "${temporary}"' EXIT

expect_accept() {
  local packaged="$1"
  local installed="$2"
  local required="$3"

  if ! bash "${helper}" "${packaged}" "${installed}" "${required}" >/dev/null; then
    echo "expected version proof to accept package=${packaged}, installed=${installed}, required=${required}" >&2
    exit 1
  fi
}

expect_reject() {
  local packaged="$1"
  local installed="$2"
  local required="$3"

  if bash "${helper}" "${packaged}" "${installed}" "${required}" >/dev/null 2>&1; then
    echo "expected version proof to reject package=${packaged}, installed=${installed}, required=${required}" >&2
    exit 1
  fi
}

# Security-floor boundaries.
expect_reject "2.52.5" "2.52.5" "2.52.6"
expect_accept "2.52.6" "2.52.6" "2.52.6"
expect_reject "2.52.4" "2.52.4" "2.52.5"
expect_accept "2.52.5" "2.52.5" "2.52.5"
expect_accept "2.52.6" "2.52.6" "2.52.5"
expect_accept "2.52.100000000000000000000" "2.52.100000000000000000000" "2.52.5"

# Only the explicitly reviewed stable release line is admissible.
expect_reject "2.51.99" "2.51.99" "2.52.5"
expect_reject "2.53.0" "2.53.0" "2.52.5"
expect_reject "2.52.5" "2.52.6" "2.52.5"

# The independently reviewed line cannot be changed through the floor alone.
expect_reject "2.53.0" "2.53.0" "2.53.0"
expect_reject "3.52.5" "3.52.5" "3.52.5"

# Malformed and potentially executable spellings remain inert and fail closed.
marker="${temporary}/injected"
malicious_required='2.52.$(touch '"${marker}"')'
expect_reject "2.52.5-1.fc44" "2.52.5-1.fc44" "2.52.5"
expect_reject "02.52.5" "02.52.5" "2.52.5"
expect_reject "2.52" "2.52" "2.52.5"
expect_reject "2.52.5;touch ${marker}" "2.52.5;touch ${marker}" "2.52.5"
expect_reject "2.52.5" "2.52.5" "${malicious_required}"
test ! -e "${marker}"

# Every native CI/release proof, including the post-bundle publication check,
# must use the same data-only comparator. Reintroducing RPM capability syntax
# here would revive the equality-floor false rejection this helper replaced.
ci_workflow="${root}/.github/workflows/ci.yml"
release_workflow="${root}/.github/workflows/build.yml"
test "$(grep -Fc 'bash scripts/ci/verify_webkitgtk_version.sh' "${ci_workflow}")" -eq 1
test "$(grep -Fc 'bash scripts/ci/verify_webkitgtk_version.sh' "${release_workflow}")" -eq 2
if grep -F 'rpm -q --whatprovides "webkit2gtk4.1 >=' \
  "${ci_workflow}" "${release_workflow}" >/dev/null; then
  echo "native WebKitGTK gates must not use RPM capability comparisons" >&2
  exit 1
fi

echo "WebKitGTK version proof tests passed"
