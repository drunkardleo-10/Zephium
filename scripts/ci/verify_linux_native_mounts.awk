# Bounded topology evidence only. Never print mount sources or host paths.
function option(options, value) {
  return index("," options ",", "," value ",") != 0
}
{
  if (NR > 256 || length($0) > 4096) { invalid = 1; exit 1 }
  if ($5 == "/") root_ro += option($6, "ro")
  if ($5 == "/workspace") workspace_ro += option($6, "ro")
  if ($5 == "/sys") sys_ro += option($6, "ro")
  if ($5 ~ /^\/proc\//) proc_children++
  if ($5 == "/proc") {
    proc_count++
    split($0, parts, " - ")
    split(parts[2], filesystem, " ")
    proc_valid += $4 == "/" && filesystem[1] == "proc" &&
      option($6, "rw") && option($6, "nosuid") &&
      option($6, "nodev") && option($6, "noexec")
  }
}
END {
  valid = !invalid && root_ro == 1 && workspace_ro == 1 && sys_ro == 1 &&
    proc_count == 1 && proc_valid == 1 && proc_children == 0
  printf "native mount topology: root_ro=%d workspace_ro=%d sys_ro=%d proc=%d proc_valid=%d proc_children=%d bounded=%d\n", root_ro, workspace_ro, sys_ro, proc_count, proc_valid, proc_children, !invalid
  if (!valid) exit 1
}
