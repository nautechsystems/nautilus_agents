#!/usr/bin/env bash
# Enforces anyhow usage conventions:
# 1. Only import anyhow::Context (use anyhow::bail!, anyhow::Result, etc. fully qualified)
# 2. Use anyhow::bail!(...) instead of return Err(anyhow::anyhow!(...))

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping anyhow usage checks"
  exit 0
fi

RED='\033[0;31m'
NC='\033[0m'

VIOLATIONS=0

PATTERN='^[[:space:]]*use anyhow::'

while IFS=: read -r file line_num line_content; do
  [[ -z "$file" ]] && continue

  normalized=$(echo "$line_content" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*\/\/.*//' -e 's/[[:space:]]*$//')

  if [[ "$normalized" == "use anyhow::Context;" ]]; then
    continue
  fi

  echo -e "${RED}Error:${NC} Invalid anyhow import in $file:$line_num"
  echo "  Found: $line_content"
  echo "  Only 'use anyhow::Context;' is allowed."
  echo "  Use fully qualified paths for other items (anyhow::bail!, anyhow::Result, etc.)"
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
done < <(rg -n "$PATTERN" src --type rust 2> /dev/null || true)

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS anyhow import violation(s)${NC}"
  exit 1
fi

USAGE_VIOLATIONS=0

while IFS=: read -r file line_num line_content; do
  [[ -z "$file" ]] && continue

  echo -e "${RED}Error:${NC} Use anyhow::bail! instead of return Err(anyhow::anyhow!) in $file:$line_num"
  echo "  Found: $line_content"
  echo "  Replace 'return Err(anyhow::anyhow!(...))' with 'anyhow::bail!(...)'"
  echo
  USAGE_VIOLATIONS=$((USAGE_VIOLATIONS + 1))
done < <(rg -n 'return\s+Err\(anyhow::anyhow!' src --type rust 2> /dev/null || true)

if [ $USAGE_VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $USAGE_VIOLATIONS anyhow usage violation(s)${NC}"
  exit 1
fi

echo "All anyhow imports and usage are valid"
exit 0
