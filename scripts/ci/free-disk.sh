#!/usr/bin/env bash
# Free room on a standard GitHub-hosted Ubuntu runner before a big build. The image carries about
# 30 GB of toolchains this repository never uses (.NET, the Android SDK, GHC, CodeQL's bundle, the
# hosted tool cache's Swift and Go); a workspace build of ~650 crates plus the sccache directory can
# fill the rest, and a runner whose disk is full stops answering ("The hosted runner lost
# communication with the server") with no log of why. Best effort: where a path or sudo is missing
# it carries on, and it never fails the job.
set -uo pipefail
before=$(df -BG --output=avail / 2>/dev/null | tail -n 1 | tr -dc '0-9')
for d in /usr/share/dotnet /usr/local/lib/android /opt/ghc /usr/local/.ghcup /opt/hostedtoolcache/CodeQL /usr/local/share/boost /usr/share/swift; do
  if [ -d "$d" ]; then sudo rm -rf "$d" 2>/dev/null || true; fi
done
after=$(df -BG --output=avail / 2>/dev/null | tail -n 1 | tr -dc '0-9')
echo "free-disk: ${before:-?} GB free before, ${after:-?} GB after"
df -h / /mnt 2>/dev/null || true
exit 0
