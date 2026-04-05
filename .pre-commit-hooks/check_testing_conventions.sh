#!/usr/bin/env bash
# Enforces testing conventions:
# 1. Rust: Prefer #[rstest] over #[test] for consistency and parametrization support
# 2. No AAA-style comments (Arrange/Act/Assert) in Rust tests

set -euo pipefail

RED='\033[0;31m'
YELLOW='\033[0;33m'
NC='\033[0m'

VIOLATIONS=0

if ! command -v rg &> /dev/null; then
  echo -e "${YELLOW}WARNING: ripgrep (rg) not found, skipping testing convention checks${NC}"
  exit 0
fi

echo "Checking Rust testing conventions..."

rust_results=$(mktemp)
aaa_results=$(mktemp)
trap 'rm -f "$rust_results" "$aaa_results"' EXIT

rg -n '^\s*#\[test\]' src --type rust 2> /dev/null > "$rust_results" || true

while IFS=: read -r file line_num line_content; do
  [[ -z "$file" ]] && continue

  trimmed_line="${line_content#"${line_content%%[![:space:]]*}"}"

  echo -e "${RED}Error:${NC} Found #[test] instead of #[rstest] in $file:$line_num"
  echo "  Found: $trimmed_line"
  echo "  Expected: #[rstest]"
  echo "  Reason: Use #[rstest] for consistency and parametrization support"
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
done < "$rust_results"

echo "Checking for AAA-style comments in Rust tests..."

rg -n '^\s*//\s*(Arrange|Act|Assert)\s*($|:|\s*-)' src --type rust 2> /dev/null > "$aaa_results" || true

while IFS=: read -r file line_num line_content; do
  [[ -z "$file" ]] && continue

  trimmed_line="${line_content#"${line_content%%[![:space:]]*}"}"

  echo -e "${RED}Error:${NC} Found AAA-style comment in $file:$line_num"
  echo "  Found: $trimmed_line"
  echo "  Reason: Arrange/Act/Assert comments are a Python convention, not used in Rust tests"
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
done < "$aaa_results"

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS testing convention violation(s)${NC}"
  echo
  echo "Convention:"
  echo "  - Rust: Use #[rstest] instead of #[test] for consistency"
  echo "  - #[tokio::test] is acceptable for async tests without parametrization"
  echo "  - Do not use // Arrange / // Act / // Assert comments in Rust tests"
  exit 1
fi

echo "All testing conventions are valid"
exit 0
