#!/usr/bin/env bash
set +x
set +a
set -euo pipefail

# Secrets arrive through the Actions environment. Copy them into non-exported
# shell variables, then remove them from every child process environment before
# invoking even diagnostic tooling.
: "${RPM_SIGNING_PRIVATE_KEY:?RPM_SIGNING_PRIVATE_KEY is required}"
: "${RPM_SIGNING_KEY_PASSPHRASE:?RPM_SIGNING_KEY_PASSPHRASE is required}"
: "${RPM_SIGNING_KEY_FINGERPRINT:?RPM_SIGNING_KEY_FINGERPRINT is required}"
signing_private_key="${RPM_SIGNING_PRIVATE_KEY}"
signing_passphrase="${RPM_SIGNING_KEY_PASSPHRASE}"
signing_fingerprint="${RPM_SIGNING_KEY_FINGERPRINT}"
unset RPM_SIGNING_PRIVATE_KEY RPM_SIGNING_KEY_PASSPHRASE
unset CDPATH GNUPGHOME GPG_AGENT_INFO GPG_TTY RPM_CONFIGDIR RPM_MACROS

export LANG=C
export LC_ALL=C
export TZ=UTC
umask 077

if [[ "$#" -ne 2 ]]; then
  echo "usage: sign_rpm.sh PACKAGE.rpm PUBLIC_KEY.asc" >&2
  exit 64
fi

rpm_path="$1"
public_key_path="$2"
if [[ ! "${signing_fingerprint}" =~ ^[0-9A-F]{40}$ ]]; then
  echo "RPM_SIGNING_KEY_FINGERPRINT must be an uppercase 40-hex fingerprint" >&2
  exit 65
fi
if [[ "${signing_passphrase}" == *$'\n'* || "${signing_passphrase}" == *$'\r'* ]]; then
  echo "RPM signing passphrase must be a single exact line" >&2
  exit 65
fi
if [[ ! -f "${rpm_path}" || -L "${rpm_path}" ]]; then
  echo "refusing to sign a missing, non-regular, or symbolic-link RPM" >&2
  exit 65
fi
if [[ -e "${public_key_path}" || -L "${public_key_path}" ]]; then
  echo "refusing to replace an existing RPM public-key output" >&2
  exit 65
fi

rpm_parent_input="$(dirname -- "${rpm_path}")"
public_parent_input="$(dirname -- "${public_key_path}")"
if [[ ! -d "${rpm_parent_input}" || -L "${rpm_parent_input}" ]]; then
  echo "RPM parent must be a non-symbolic directory" >&2
  exit 65
fi
if [[ ! -d "${public_parent_input}" || -L "${public_parent_input}" ]]; then
  echo "RPM public-key parent must be a non-symbolic directory" >&2
  exit 65
fi
rpm_dir="$(cd -P -- "${rpm_parent_input}" && pwd)"
public_key_dir="$(cd -P -- "${public_parent_input}" && pwd)"
rpm_name="$(basename -- "${rpm_path}")"
public_key_name="$(basename -- "${public_key_path}")"
if [[ -z "${rpm_name}" || "${rpm_name}" == "." || "${rpm_name}" == ".." ||
      -z "${public_key_name}" || "${public_key_name}" == "." || "${public_key_name}" == ".." ]]; then
  echo "RPM input or public-key output has an invalid file name" >&2
  exit 65
fi
rpm_path="${rpm_dir}/${rpm_name}"
public_key_path="${public_key_dir}/${public_key_name}"
if [[ ! -f "${rpm_path}" || -L "${rpm_path}" ||
      -e "${public_key_path}" || -L "${public_key_path}" ]]; then
  echo "RPM input or public-key output changed during path validation" >&2
  exit 65
fi

work_dir=""
publication_dir=""
key_file=""
passphrase_file=""
staged_public_key=""
public_key_published=0
committed=0

cleanup() {
  local status=$?
  set +e
  if [[ "${public_key_published}" -eq 1 && "${committed}" -eq 0 &&
        -n "${staged_public_key}" && -f "${staged_public_key}" &&
        ! -L "${public_key_path}" && -f "${public_key_path}" &&
        "${staged_public_key}" -ef "${public_key_path}" ]]; then
    rm -f -- "${public_key_path}"
    sync -f -- "${public_key_dir}" >/dev/null 2>&1 || true
  fi
  for secret_file in "${key_file}" "${passphrase_file}"; do
    if [[ -n "${secret_file}" && -f "${secret_file}" && ! -L "${secret_file}" ]]; then
      shred -u -- "${secret_file}" >/dev/null 2>&1 || rm -f -- "${secret_file}"
    fi
  done
  if [[ -n "${GNUPGHOME:-}" && -d "${GNUPGHOME}" && ! -L "${GNUPGHOME}" ]]; then
    gpgconf --homedir "${GNUPGHOME}" --kill gpg-agent >/dev/null 2>&1 || true
  fi
  if [[ -n "${work_dir}" && -d "${work_dir}" && ! -L "${work_dir}" ]]; then
    chmod -R u+w -- "${work_dir}" >/dev/null 2>&1 || true
    rm -rf -- "${work_dir}"
  fi
  if [[ -n "${publication_dir}" && -d "${publication_dir}" && ! -L "${publication_dir}" ]]; then
    chmod -R u+w -- "${publication_dir}" >/dev/null 2>&1 || true
    rm -rf -- "${publication_dir}"
  fi
  trap - EXIT HUP INT TERM
  exit "${status}"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

work_dir="$(mktemp -d "${rpm_dir}/.zephium-rpm-sign.XXXXXXXX")"
publication_dir="$(mktemp -d "${public_key_dir}/.zephium-rpm-key.XXXXXXXX")"
chmod 700 -- "${work_dir}" "${publication_dir}"
export GNUPGHOME="${work_dir}/gnupg"
mkdir -m 700 -- "${GNUPGHOME}"
export HOME="${work_dir}/home"
mkdir -m 700 -- "${HOME}"

file_identity() {
  stat --format='%d:%i:%s:%Y:%Z:%f' -- "$1"
}

file_digest() {
  local output digest
  output="$(sha256sum -- "$1")"
  digest="${output%% *}"
  if [[ ! "${digest}" =~ ^[0-9a-f]{64}$ ]]; then
    echo "failed to obtain an unambiguous SHA-256 digest for $1" >&2
    return 1
  fi
  printf '%s\n' "${digest}"
}

original_identity="$(file_identity "${rpm_path}")"
original_digest="$(file_digest "${rpm_path}")"
original_mode="$(stat --format='%a' -- "${rpm_path}")"
if [[ ! "${original_mode}" =~ ^[0-7]{3,4}$ ]]; then
  echo "failed to preserve the RPM file mode" >&2
  exit 65
fi

# Sign only a mode-0600 copy in a mode-0700 directory on the destination
# filesystem. The caller-owned RPM is never passed to rpmsign.
staged_rpm="${work_dir}/package.rpm"
cp --no-dereference -- "${rpm_path}" "${staged_rpm}"
chmod 600 -- "${staged_rpm}"
if [[ ! -f "${staged_rpm}" || -L "${staged_rpm}" ||
      "$(file_identity "${rpm_path}")" != "${original_identity}" ||
      "$(file_digest "${rpm_path}")" != "${original_digest}" ||
      "$(file_digest "${staged_rpm}")" != "${original_digest}" ]]; then
  echo "RPM changed while its private signing snapshot was created" >&2
  exit 65
fi

key_file="${work_dir}/private-key.asc"
passphrase_file="${work_dir}/passphrase"
printf '%s' "${signing_private_key}" > "${key_file}"
printf '%s' "${signing_passphrase}" > "${passphrase_file}"
chmod 600 -- "${key_file}" "${passphrase_file}"
unset signing_private_key signing_passphrase

gpg --batch --import-options import-minimal --import "${key_file}"
shred -u -- "${key_file}"
key_file=""

key_listing="${work_dir}/secret-keys.colons"
gpg --batch --fixed-list-mode --with-colons --list-secret-keys > "${key_listing}"
mapfile -t fingerprints < <(
  awk -F: '
    $1 == "sec" { want = 1; next }
    want && $1 == "fpr" { print toupper($10); want = 0 }
  ' "${key_listing}"
)
if [[ "${#fingerprints[@]}" -ne 1 || "${fingerprints[0]}" != "${signing_fingerprint}" ]]; then
  echo "RPM signing secret does not contain exactly the configured signing key" >&2
  exit 66
fi

now="$(date +%s)"
if ! awk -F: -v now="${now}" '
  function unusable(validity, expiry, capabilities) {
    return validity ~ /[reid]/ || capabilities ~ /D/ ||
      (expiry != "" && expiry != "0" && expiry <= now) ||
      capabilities !~ /[sS]/
  }
  $1 == "sec" {
    primary += 1
    if ($2 ~ /[reid]/ || $12 ~ /D/ ||
        ($7 != "" && $7 != "0" && $7 <= now)) primary_bad = 1
  }
  ($1 == "sec" || $1 == "ssb") && !unusable($2, $7, $12) {
    usable_signer = 1
  }
  END { exit !(primary == 1 && !primary_bad && usable_signer) }
' "${key_listing}"; then
  echo "RPM signing key is revoked, disabled, invalid, expired, or has no usable signing capability" >&2
  exit 66
fi

# Prove both key usability and the passphrase before asking RPM to modify its
# private copy. Verification is local to this isolated keyring.
challenge="${work_dir}/signing-challenge"
challenge_signature="${work_dir}/signing-challenge.sig"
printf 'Zephium RPM signing-key proof\n' > "${challenge}"
gpg --batch --yes --pinentry-mode loopback \
  --passphrase-file "${passphrase_file}" \
  --local-user "${signing_fingerprint}" \
  --output "${challenge_signature}" --detach-sign "${challenge}"
gpg --batch --verify "${challenge_signature}" "${challenge}"

staged_public_key="${publication_dir}/${public_key_name}"
gpg --batch --yes --output "${staged_public_key}" \
  --armor --export "${signing_fingerprint}"
if [[ ! -s "${staged_public_key}" || -L "${staged_public_key}" ]]; then
  echo "GPG did not emit a regular non-empty RPM public key" >&2
  exit 66
fi
mapfile -t public_fingerprints < <(
  gpg --batch --fixed-list-mode --with-colons --show-keys "${staged_public_key}" |
    awk -F: '
      $1 == "pub" { want = 1; next }
      want && $1 == "fpr" { print toupper($10); want = 0 }
    '
)
if [[ "${#public_fingerprints[@]}" -ne 1 ||
      "${public_fingerprints[0]}" != "${signing_fingerprint}" ]]; then
  echo "exported RPM public key does not match the configured fingerprint" >&2
  exit 66
fi

printf -v passphrase_path_for_macro '%q' "${passphrase_file}"
rpmsign --addsign \
  --define "_gpg_name ${signing_fingerprint}" \
  --define "_gpg_path ${GNUPGHOME}" \
  --define "_gpg_digest_algo sha256" \
  --define "__gpg /usr/bin/gpg2" \
  --define "__gpg_sign_cmd %{__gpg} --batch --no-verbose --no-armor --pinentry-mode loopback --passphrase-file ${passphrase_path_for_macro} --no-secmem-warning --digest-algo sha256 --local-user %{_gpg_name} --sign --detach-sign --output %{__signature_filename} %{__plaintext_filename}" \
  "${staged_rpm}"
shred -u -- "${passphrase_file}"
passphrase_file=""

if [[ ! -f "${staged_rpm}" || -L "${staged_rpm}" ]]; then
  echo "rpmsign did not preserve a regular private RPM copy" >&2
  exit 67
fi
chmod "${original_mode}" -- "${staged_rpm}"
verify_db="${work_dir}/rpmdb"
mkdir -m 700 -- "${verify_db}"
rpmkeys --dbpath "${verify_db}" --import "${staged_public_key}"
verification="$(rpmkeys --dbpath "${verify_db}" --checksig --verbose "${staged_rpm}")"
printf '%s\n' "${verification}"
if [[ ! "${verification}" =~ digests[[:space:]]signatures[[:space:]]OK$ ]]; then
  echo "RPM publisher-signature verification failed" >&2
  exit 67
fi

# No verification failure may replace the caller's RPM. Revalidate its exact
# path/inode/bytes only after the signed copy has passed a fresh-keyring check.
if [[ ! -f "${rpm_path}" || -L "${rpm_path}" ||
      "$(file_identity "${rpm_path}")" != "${original_identity}" ||
      "$(file_digest "${rpm_path}")" != "${original_digest}" ]]; then
  echo "original RPM changed before transactional signing commit" >&2
  exit 68
fi

chmod 644 -- "${staged_public_key}"
sync -f -- "${staged_public_key}"
# Hard-link publication is an atomic exclusive create of the exact verified
# inode. It refuses regular files, dangling links, and symbolic links alike.
ln -- "${staged_public_key}" "${public_key_path}"
public_key_published=1
if [[ -L "${public_key_path}" || ! -f "${public_key_path}" ||
      ! "${staged_public_key}" -ef "${public_key_path}" ||
      "$(stat --format='%a' -- "${public_key_path}")" != "644" ]]; then
  echo "RPM public key changed during exclusive publication" >&2
  exit 68
fi
sync -f -- "${public_key_dir}"
sync -f -- "${staged_rpm}"

if [[ ! -f "${rpm_path}" || -L "${rpm_path}" ||
      "$(file_identity "${rpm_path}")" != "${original_identity}" ||
      "$(file_digest "${rpm_path}")" != "${original_digest}" ||
      ! -f "${staged_rpm}" || -L "${staged_rpm}" ||
      -L "${public_key_path}" || ! -f "${public_key_path}" ||
      ! "${staged_public_key}" -ef "${public_key_path}" ]]; then
  echo "RPM or public key changed at transactional signing commit" >&2
  exit 68
fi

# work_dir is on the same filesystem as rpm_path, so GNU mv commits with one
# atomic rename only after every signing and verification gate has succeeded.
mv -fT -- "${staged_rpm}" "${rpm_path}"
committed=1
# The atomic commit is already complete. Flush the filesystem where supported;
# a sync diagnostic must not misreport the valid committed artifact as unsigned.
sync -f -- "${rpm_dir}" >/dev/null 2>&1 || true
