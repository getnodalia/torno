#!/usr/bin/env bash
# Give a standard 4 vCPU / 16 GB runner a swap file, so a burst of memory (a handful of test
# processes each drawing on lavapipe, or the action matrix's workers) slows the job down instead of
# starving the runner agent: "The hosted runner lost communication with the server" is what GitHub
# prints when the machine runs out of memory and the agent stops answering, and the job dies with no
# log. The file goes on the runner's temporary disk (/mnt) so the build's own disk is left alone.
# It is an optimisation and must never fail the job: where there is no sudo, no room or no swapon
# (a sandbox, a self-hosted box), it says so and exits 0.
# Usage: add-swap.sh [SIZE_GB] (default 8)
set -uo pipefail
want=${1:-8}
dir=/mnt
if ! command -v sudo >/dev/null 2>&1 || ! [ -d "$dir" ]; then
  echo "add-swap: no sudo or no $dir here; leaving swap as it is"
  exit 0
fi
free_gb=$(df -BG --output=avail "$dir" 2>/dev/null | tail -n 1 | tr -dc '0-9')
case "$free_gb" in
  '' ) echo "add-swap: cannot read the free space of $dir; leaving swap as it is"; exit 0 ;;
esac
# Keep 2 GB free on the disk for whatever else the job puts there.
size=$((free_gb - 2))
if [ "$size" -gt "$want" ]; then size=$want; fi
if [ "$size" -lt 2 ]; then
  echo "add-swap: only ${free_gb} GB free on $dir; leaving swap as it is"
  exit 0
fi
file="$dir/torno-ci.swap"
if sudo swapoff "$file" 2>/dev/null; then :; fi
if sudo fallocate -l "${size}G" "$file" \
  && sudo chmod 600 "$file" \
  && sudo mkswap "$file" >/dev/null \
  && sudo swapon "$file"; then
  echo "add-swap: ${size} GB of swap on $file"
else
  echo "add-swap: could not enable swap; carrying on without it"
  sudo rm -f "$file" 2>/dev/null || true
fi
free -m || true
exit 0
