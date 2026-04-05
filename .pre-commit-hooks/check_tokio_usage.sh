#!/usr/bin/env bash
# Enforces tokio usage conventions:
# 1. tokio::time::*, tokio::spawn, tokio::sync::* should be fully qualified
#
# Use '// tokio-import-ok' comment to allow specific exceptions

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping tokio usage checks"
  exit 0
fi

RED='\033[0;31m'
NC='\033[0m'

VIOLATIONS=0

ALLOW_MARKER="tokio-import-ok"

echo "Checking tokio import conventions..."

# Check for use tokio::time::* imports
while IFS=: read -r file line_num line_content; do
  [[ -z "$file" ]] && continue
  [[ "$line_content" =~ $ALLOW_MARKER ]] && continue

  trimmed="${line_content#"${line_content%%[![:space:]]*}"}"

  if [[ "$line_content" =~ Duration ]]; then
    echo -e "${RED}Error:${NC} Use std::time::Duration instead of tokio::time::Duration in $file:$line_num"
    echo "  Found: $trimmed"
    echo "  tokio::time::Duration is just a re-export of std::time::Duration"
  else
    echo -e "${RED}Error:${NC} tokio::time should be fully qualified in $file:$line_num"
    echo "  Found: $trimmed"
    echo "  Use tokio::time::sleep, tokio::time::timeout, etc. inline instead of importing"
  fi
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
done < <(rg -n "^[[:space:]]*use tokio::time::" src --type rust 2> /dev/null || true)

# Check for use tokio::spawn imports
while IFS=: read -r file line_num line_content; do
  [[ -z "$file" ]] && continue
  [[ "$line_content" =~ $ALLOW_MARKER ]] && continue

  trimmed="${line_content#"${line_content%%[![:space:]]*}"}"
  echo -e "${RED}Error:${NC} tokio::spawn should be fully qualified in $file:$line_num"
  echo "  Found: $trimmed"
  echo "  Use tokio::spawn(...) inline instead of importing"
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
done < <(rg -n "^[[:space:]]*use tokio::spawn" src --type rust 2> /dev/null || true)

# Check for use tokio::try_join imports
while IFS=: read -r file line_num line_content; do
  [[ -z "$file" ]] && continue
  [[ "$line_content" =~ $ALLOW_MARKER ]] && continue

  trimmed="${line_content#"${line_content%%[![:space:]]*}"}"
  echo -e "${RED}Error:${NC} tokio::try_join should be fully qualified in $file:$line_num"
  echo "  Found: $trimmed"
  echo "  Use tokio::try_join!(...) inline instead of importing"
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
done < <(rg -n "^[[:space:]]*use tokio::try_join" src --type rust 2> /dev/null || true)

# Check for use tokio::sync::* imports
while IFS=: read -r file line_num line_content; do
  [[ -z "$file" ]] && continue
  [[ "$line_content" =~ $ALLOW_MARKER ]] && continue

  trimmed="${line_content#"${line_content%%[![:space:]]*}"}"
  echo -e "${RED}Error:${NC} tokio::sync should be fully qualified in $file:$line_num"
  echo "  Found: $trimmed"
  echo "  Use tokio::sync::Mutex, tokio::sync::RwLock, etc. inline instead of importing"
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
done < <(rg -n "^[[:space:]]*use tokio::sync::" src --type rust 2> /dev/null || true)

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS tokio usage violation(s)${NC}"
  echo
  echo "Add '// tokio-import-ok' comment to allow specific exceptions"
  exit 1
fi

echo "All tokio usage is valid"
exit 0
