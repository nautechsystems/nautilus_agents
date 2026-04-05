#!/usr/bin/env bash

# Check for TODO! patterns that shouldn't be committed
set -e

matches=$(grep -R --binary-files=without-match -n "TODO!" \
  --exclude-dir=.git \
  --exclude-dir=target \
  --exclude-dir=build \
  --exclude-dir=node_modules \
  --exclude=".pre-commit-config.yaml" \
  --exclude-dir=.pre-commit-hooks \
  . || true)

if [[ -n "$matches" ]]; then
  count=$(echo "$matches" | wc -l)
  if [[ $count -eq 1 ]]; then
    echo "TODO! marker detected (should not be committed):"
    echo "$matches"
    echo ""
    echo "Please resolve this TODO! marker before committing."
  else
    echo "TODO! markers detected (should not be committed):"
    echo "$matches"
    echo ""
    echo "Please resolve these TODO! markers before committing."
  fi
  exit 1
fi
