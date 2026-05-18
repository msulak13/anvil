#!/usr/bin/env bash
# Pack (handles workspace:* → real version) then publish via OIDC.
# Treats "version already published" as success so reruns are safe.
set -e
pnpm pack
tgz=$(ls *.tgz | head -1)
npm publish "$tgz" --provenance --access public || {
  code=$?
  pkg=$(jq -r .name package.json)
  ver=$(jq -r .version package.json)
  if npm show "${pkg}@${ver}" version 2>/dev/null | grep -qF "$ver"; then
    echo "Already published ${pkg}@${ver}, skipping"
    exit 0
  fi
  exit $code
}
