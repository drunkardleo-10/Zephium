# Bounded topology evidence only. Never print mount sources or host paths.
function option(options, value) {
  return index("," options ",", "," value ",") != 0
}
{
  if (NR > 256 || length($0) > 4096) { invalid = 1; exit 1 }
  split($0, parts, " - ")
  split(parts[2], filesystem, " ")
  if ($5 == "/") { root_count++; root_ro += option($6, "ro") }
  if ($5 == "/workspace") { workspace_count++; workspace_ro += option($6, "ro") }
  if ($5 == "/sys") { sys_count++; sys_ro += option($6, "ro") }
  if ($5 == "/sys/firmware" || $5 == "/sys/devices/virtual/powercap") {
    mask_valid = $4 == "/" && filesystem[1] == "tmpfs" &&
      option($6, "ro") && option($6, "nosuid") &&
      option($6, "nodev") && option($6, "noexec")
    if ($5 == "/sys/firmware") { firmware_count++; firmware_valid += mask_valid }
    else { powercap_count++; powercap_valid += mask_valid }
  }
  if ($5 ~ /^\/sys\/firmware\// || $5 ~ /^\/sys\/devices\/virtual\/powercap\//) invalid = 1
  if ($5 ~ /^\/proc\//) proc_children++
  if ($5 == "/proc") {
    proc_count++
    proc_valid += $4 == "/" && filesystem[1] == "proc" &&
      option($6, "rw") && option($6, "nosuid") &&
      option($6, "nodev") && option($6, "noexec")
  }
}
END {
  expected_powercap = powercap_present == "true" ? 1 : 0
  valid = !invalid && (powercap_present == "true" || powercap_present == "false") &&
    root_count == 1 && root_ro == 1 && workspace_count == 1 && workspace_ro == 1 &&
    sys_count == 1 && sys_ro == 1 && firmware_count == 1 && firmware_valid == 1 &&
    powercap_count == expected_powercap && powercap_valid == expected_powercap &&
    proc_count == 1 && proc_valid == 1 && proc_children == 0
  printf "native mount topology: root_count=%d root_ro=%d workspace_count=%d workspace_ro=%d sys_count=%d sys_ro=%d firmware=%d firmware_valid=%d powercap=%d powercap_valid=%d proc=%d proc_valid=%d proc_children=%d bounded=%d\n", root_count, root_ro, workspace_count, workspace_ro, sys_count, sys_ro, firmware_count, firmware_valid, powercap_count, powercap_valid, proc_count, proc_valid, proc_children, !invalid
  if (!valid) exit 1
}
