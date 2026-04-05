#!/usr/bin/env bash
# Enforces Cargo.toml conventions:
# 1. Dependencies within groups (separated by blank lines) must be alphabetically ordered
# 2. Sections must be in standard order

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping Cargo convention checks"
  exit 0
fi

RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "Checking Cargo.toml conventions..."

VIOLATIONS=0

# Check 1: Dependency ordering within groups
# shellcheck disable=SC2016
dep_violations=$(rg --files -g "Cargo.toml" --glob "!target/*" 2> /dev/null | sort | xargs awk '
BEGIN {
  in_deps = 0
  section = ""
  prev_name = ""
  prev_line = 0
}

/^\[+[a-zA-Z0-9._-]+\]+$/ {
  prev_name = ""
  prev_line = 0
  gsub(/^\[+|\]+$/, "", $0)
  if ($0 == "dependencies" || $0 == "dev-dependencies" || $0 == "build-dependencies" || $0 == "workspace.dependencies") {
    in_deps = 1
    section = $0
  } else {
    in_deps = 0
    section = ""
  }
  next
}

in_deps && /^[[:space:]]*$/ {
  prev_name = ""
  prev_line = 0
  next
}

in_deps && /^[[:space:]]*#/ { next }

in_deps && /^[a-zA-Z0-9_-]+[[:space:]]*[.=]/ {
  match($0, /^[a-zA-Z0-9_-]+/)
  name = substr($0, RSTART, RLENGTH)
  name_lower = tolower(name)

  if (prev_name != "" && name_lower < tolower(prev_name)) {
    printf "  %s:%d [%s] \047%s\047 should come before \047%s\047 (line %d)\n", FILENAME, NR, section, name, prev_name, prev_line
  }

  prev_name = name
  prev_line = NR
}
' 2>&1) || true

if [[ -n "$dep_violations" ]]; then
  echo -e "${RED}Dependency ordering violations:${NC}"
  echo "$dep_violations"
  echo
  VIOLATIONS=$((VIOLATIONS + $(echo "$dep_violations" | wc -l)))
fi

# Check 2: Section ordering
section_violations=$(rg --files -g "Cargo.toml" --glob "!target/*" src/ 2> /dev/null | while read -r file; do
  awk '
  BEGIN {
    order_map["package"] = 1
    order_map["lints"] = 2
    order_map["lib"] = 3
    order_map["features"] = 4
    order_map["dependencies"] = 5
    order_map["dev-dependencies"] = 6
    order_map["build-dependencies"] = 7
    order_map["bench"] = 8
    order_map["bin"] = 9
    order_map["example"] = 10
    order_map["test"] = 11
    prev_section = ""
    prev_idx = 0
  }

  /^\[+[a-zA-Z0-9._-]+\]+$/ {
    section = $0
    gsub(/^\[+|\]+$/, "", section)

    if (section in order_map) {
      idx = order_map[section]
      if (prev_idx > 0 && idx < prev_idx) {
        printf "  %s:%d [%s] should come before [%s]\n", FILENAME, NR, section, prev_section
      }
      prev_section = section
      prev_idx = idx
    }
  }
  ' "$file"
done) || true

if [[ -n "$section_violations" ]]; then
  echo -e "${RED}Section ordering violations:${NC}"
  echo "$section_violations"
  echo
  VIOLATIONS=$((VIOLATIONS + $(echo "$section_violations" | wc -l)))
fi

if [[ $VIOLATIONS -gt 0 ]]; then
  echo -e "${RED}Found $VIOLATIONS Cargo.toml convention violation(s)${NC}"
  echo
  echo -e "${YELLOW}To fix:${NC}"
  echo "  - Sort dependencies alphabetically within each group (groups separated by blank lines)"
  echo "  - Order sections: [package], [lints], [lib], [features],"
  echo "    [dependencies], [dev-dependencies], [build-dependencies]"
  exit 1
fi

echo "All Cargo.toml conventions are valid"
exit 0
