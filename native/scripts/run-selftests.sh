#!/usr/bin/env bash
# Linux runner for the same Qt self-tests as run-selftests.ps1.
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
native_root=$(dirname -- "$script_dir")
profile=${1:-debug}
case "$profile" in debug|release) ;; *) echo 'Profile must be debug or release' >&2; exit 2 ;; esac
binary="$native_root/target/$profile/native"
test -x "$binary" || { echo "Build $binary first" >&2; exit 1; }
export QT_QPA_PLATFORM=${QT_QPA_PLATFORM:-offscreen}
log_dir="$native_root/target/selftests-$QT_QPA_PLATFORM"
mkdir -p "$log_dir"
failed=0
count=0
cd -- "$(dirname -- "$native_root")"
while IFS= read -r flag || [ -n "$flag" ]; do
  flag=${flag%$'\r'}
  [ -n "$flag" ] || continue
  count=$((count + 1))
  log="$log_dir/self-$flag.log"
  status=0
  timeout 120s "$binary" "$flag" >"$log" 2>"$log.err" || status=$?
  printf '%-22s exit=%s ' "$flag" "$status"
  tail -n 2 "$log" | tr '\n' ' '
  printf '\n'
  if [ "$status" -ne 0 ]; then failed=$((failed + 1)); fi
done <"$script_dir/selftest-flags.txt"
printf 'Qt self-tests: %s total, %s failed (%s)\n' "$count" "$failed" "$QT_QPA_PLATFORM"
test "$failed" -eq 0
