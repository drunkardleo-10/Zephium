# The host supplies at most the latest 256 records since this container start.
# A zero count is not proof of absence: kernel/audit logs can be rate-limited.
{
  if (NR > 256 || length($0) > 16384) { invalid = 1; exit 1 }
  if (!index($0, "apparmor=\"DENIED\"")) next
  exact = index($0, "profile=\"zephium-native-ci\"") != 0
  bwrap = index($0, "comm=\"bwrap\"") != 0
  if (!exact && !bwrap) next
  matched++
  if (exact) exact_profile++; else other_bwrap_profile++
  if (index($0, "operation=\"mount\"")) mount_denial++
  else if (index($0, "operation=\"userns_create\"")) userns_denial++
  else if (index($0, "operation=\"capable\"")) capability_denial++
  else other_operation++
}
END {
  printf "native host denials: sampled=%d matching=%d exact_profile=%d other_bwrap_profile=%d mount=%d userns=%d capability=%d other_operation=%d bounded=%d absence_not_proof=true\n", NR, matched, exact_profile, other_bwrap_profile, mount_denial, userns_denial, capability_denial, other_operation, !invalid
  if (invalid) exit 1
}
