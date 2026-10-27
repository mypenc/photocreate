#!/usr/bin/env bash
# Publish the interesting part of a build log to the `ci-logs` branch (readable through the API).
# usage: push-log.sh <name> <logfile>
set -u
name="$1"; file="$2"
[ -f "$file" ] || { echo "no log $file"; exit 0; }
repo="$GITHUB_REPOSITORY"
sed -i 's/\x1b\[[0-9;]*m//g' "$file"
{ grep -n -E "^(error|warning: unused|ERROR|FAILURE|e: |> Task .* FAILED)" -A14 "$file" | head -400; echo; echo "==== TAIL ===="; tail -n 120 "$file"; } > /tmp/pushlog.txt
content=$(base64 < /tmp/pushlog.txt | tr -d "\n")
sha=$(gh api "repos/$repo/contents/$name.log?ref=ci-logs" --jq .sha 2>/dev/null || true)
gh api -X PUT "repos/$repo/contents/$name.log" -f message="ci log $name $GITHUB_RUN_ID" -f content="$content" -f branch=ci-logs ${sha:+-f sha="$sha"} >/dev/null && echo "log pushed"
