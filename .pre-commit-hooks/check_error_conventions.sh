#!/usr/bin/env bash
# Enforces error variable naming conventions:
# Rust: Always use Err(e) for error variables [not Err(error), Err(err), etc.]

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping error convention checks"
  exit 0
fi

RED='\033[0;31m'
NC='\033[0m'

VIOLATIONS=0

echo "Checking Rust error variable naming..."

rust_err_output=$(rg -n 'Err\((err|error)\)|\|(err|error)\|' src --type rust 2> /dev/null || true)

if [[ -n "$rust_err_output" ]]; then
  while IFS=: read -r file line_num line_content; do
    [[ -z "$file" ]] && continue
    trimmed_line="${line_content#"${line_content%%[![:space:]]*}"}"

    if [[ "$line_content" =~ Err\((err|error)\) ]]; then
      var_name="${BASH_REMATCH[1]}"
      echo -e "${RED}Error:${NC} Invalid Rust error variable name in $file:$line_num"
      echo "  Found: Err($var_name) - use Err(e)"
      echo "  Line: $trimmed_line"
      echo
      VIOLATIONS=$((VIOLATIONS + 1))
    elif [[ "$line_content" =~ \|(err|error)\| ]]; then
      var_name="${BASH_REMATCH[1]}"
      echo -e "${RED}Error:${NC} Invalid Rust closure error variable in $file:$line_num"
      echo "  Found: |$var_name| - use |e|"
      echo "  Line: $trimmed_line"
      echo
      VIOLATIONS=$((VIOLATIONS + 1))
    fi
  done <<< "$rust_err_output"
fi

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS error variable naming violation(s)${NC}"
  echo
  echo "Convention: Use Err(e) or descriptive names (not Err(err) or Err(error))"
  exit 1
fi

echo "All error variable names are valid"
exit 0
