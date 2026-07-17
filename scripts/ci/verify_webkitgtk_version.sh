#!/usr/bin/env bash
# Verify that Fedora's WebKitGTK package and pkg-config resolve to one
# canonical, reviewed release at or above the configured security floor.

set -euo pipefail
export LC_ALL=C

readonly REVIEWED_MAJOR="2"
readonly REVIEWED_MINOR="52"

if (( $# != 3 )); then
  echo "usage: verify_webkitgtk_version.sh PACKAGED INSTALLED REQUIRED" >&2
  exit 64
fi

packaged="$1"
installed="$2"
required="$3"

parse_version() {
  local label="$1"
  local version="$2"

  # Canonical decimal components only. Besides rejecting distro suffixes and
  # prereleases, this keeps every later comparison data-only: no value is
  # evaluated as shell or RPM syntax.
  if [[ ! "${version}" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    echo "${label} must be canonical MAJOR.MINOR.PATCH, got '${version}'" >&2
    return 1
  fi

  VERSION_MAJOR="${BASH_REMATCH[1]}"
  VERSION_MINOR="${BASH_REMATCH[2]}"
  VERSION_PATCH="${BASH_REMATCH[3]}"
}

decimal_is_at_least() {
  local actual="$1"
  local floor="$2"

  # Components can be longer than the shell's integer width. Compare their
  # canonical decimal spellings by length and then lexically instead of using
  # arithmetic expansion, which could overflow or reinterpret input.
  if (( ${#actual} != ${#floor} )); then
    (( ${#actual} > ${#floor} ))
    return
  fi
  [[ "${actual}" == "${floor}" || "${actual}" > "${floor}" ]]
}

parse_version "required WebKitGTK version" "${required}"
required_major="${VERSION_MAJOR}"
required_minor="${VERSION_MINOR}"
required_patch="${VERSION_PATCH}"

# The floor and the independently reviewed stable release line are separate
# policy inputs. A mistaken workflow-environment edit must not silently admit
# a newer, unaudited WebKitGTK line.
if [[ "${required_major}" != "${REVIEWED_MAJOR}" || \
      "${required_minor}" != "${REVIEWED_MINOR}" ]]; then
  echo "required WebKitGTK version ${required} is outside the independently reviewed ${REVIEWED_MAJOR}.${REVIEWED_MINOR}.x release line" >&2
  exit 1
fi

parse_version "Fedora WebKitGTK package version" "${packaged}"
packaged_major="${VERSION_MAJOR}"
packaged_minor="${VERSION_MINOR}"
packaged_patch="${VERSION_PATCH}"

parse_version "pkg-config WebKitGTK version" "${installed}"

if [[ "${installed}" != "${packaged}" ]]; then
  echo "pkg-config resolved ${installed}, but Fedora's installed package is ${packaged}" >&2
  exit 1
fi

if [[ "${packaged_major}" != "${REVIEWED_MAJOR}" || \
      "${packaged_minor}" != "${REVIEWED_MINOR}" ]]; then
  echo "WebKitGTK ${packaged} is outside the reviewed ${REVIEWED_MAJOR}.${REVIEWED_MINOR}.x release line" >&2
  exit 1
fi

if ! decimal_is_at_least "${packaged_patch}" "${required_patch}"; then
  echo "WebKitGTK ${packaged} is below the required security floor ${required}" >&2
  exit 1
fi

echo "WebKitGTK version proof accepted: package=${packaged}; pkg-config=${installed}; required>=${required}"
