# Variables
PROJECT?=nautechsystems/nautilus_agents

V = 0
Q = $(if $(filter 1,$V),,@)
M = $(shell printf "\033[0;34m>\033[0m")

RED    := \033[0;31m
GREEN  := \033[0;32m
YELLOW := \033[0;33m
CYAN   := \033[0;36m
GRAY   := \033[0;37m
RESET  := \033[0m

.DEFAULT_GOAL := help

#== Build

.PHONY: build
build:  #-- Build in release mode
	$(info $(M) Building in release mode...)
	$Q cargo build --release

.PHONY: build-debug
build-debug:  #-- Build in debug mode
	$(info $(M) Building in debug mode...)
	$Q cargo build

#== Clean

.PHONY: clean
clean:  #-- Clean all build artifacts
	$Q cargo clean

#== Code Quality

.PHONY: format
format:  #-- Format Rust code (nightly rustfmt)
	cargo +nightly fmt --all

.PHONY: pre-commit
pre-commit:  #-- Run all pre-commit hooks on all files
	prek run --all-files

.PHONY: pre-flight
pre-flight:  #-- Run pre-commit hooks, Rust tests, and supply-chain checks
	@$(MAKE) --no-print-directory pre-commit
	@$(MAKE) --no-print-directory cargo-test
	@$(MAKE) --no-print-directory security-audit

.PHONY: check-code
check-code:  #-- Run clippy linter
	$(info $(M) Running code quality checks...)
	@cargo clippy --all-targets -- -D warnings
	@printf "$(GREEN)Checks passed$(RESET)\n"

.PHONY: clippy
clippy:  #-- Run clippy linter (check only)
	cargo clippy --all-targets -- -D warnings

.PHONY: clippy-fix
clippy-fix:  #-- Run clippy with automatic fixes
	cargo clippy --fix --all-targets --allow-dirty --allow-staged -- -D warnings

# markdownlint-cli2 version comes from the pre-commit hook rev so both agree.
MARKDOWNLINT_VERSION := $(shell awk '\
	/markdownlint-cli2/ { found=1 } \
	found && /^[[:space:]]*rev:[[:space:]]*/ { sub(/^v/, "", $$2); print $$2; exit } \
' .pre-commit-config.yaml)
MARKDOWNLINT ?= npx --yes markdownlint-cli2@$(MARKDOWNLINT_VERSION)
MARKDOWN_FILES = $(shell git ls-files '*.md')

.PHONY: check-markdown
check-markdown:  #-- Lint Markdown with markdownlint-cli2 and check table delimiter padding
	$(info $(M) Checking Markdown...)
	@$(MARKDOWNLINT) --config .markdownlint.jsonc $(MARKDOWN_FILES)
	@python3 -B scripts/check-markdown-tables.py $(MARKDOWN_FILES)
	@printf "$(GREEN)Markdown check passed$(RESET)\n"

#== Testing

.PHONY: contract-generate
contract-generate:  #-- Generate public protocol schemas and fixtures
	cargo run --locked -p agent-contract-schema -- generate

.PHONY: contract-check
contract-check:  #-- Check generated public protocol assets
	cargo run --locked -p agent-contract-schema -- check

.PHONY: cargo-test
cargo-test: export RUST_BACKTRACE=1
cargo-test:  #-- Run all Rust tests
	$(info $(M) Running Rust tests...)
	cargo test --locked --all-targets --all-features

.PHONY: cargo-check
cargo-check:  #-- Check Rust code without building
	cargo check

#== Dependencies

.PHONY: outdated
outdated: check-edit-installed  #-- Check for outdated dependencies
	cargo upgrade --dry-run --incompatible

.PHONY: update
update:  #-- Update Rust dependencies
	cargo update

#== Security

# Run an audit step quietly and display its captured output only on failure
define audit_step
	printf "$(CYAN)Running $(1)...$(RESET) "; \
	if _out=$$($(2) 2>&1); then \
		printf "$(GREEN)ok$(RESET)\n"; \
	else \
		rc=$$?; printf "$(RED)failed$(RESET)\n%s\n" "$$_out"; exit $$rc; \
	fi
endef

.PHONY: security-audit
security-audit: check-audit-installed check-deny-installed check-vet-installed  #-- Run full security audit
	$(info $(M) Running security audit...)
	@$(call audit_step,cargo audit,cargo audit --color never)
	@$(call audit_step,cargo deny,cargo deny --all-features check advisories licenses sources bans)
	@$(call audit_step,cargo vet,cargo vet --locked)

.PHONY: cargo-deny
cargo-deny: check-deny-installed  #-- Run cargo-deny checks
	cargo deny --all-features check

.PHONY: cargo-vet
cargo-vet: check-vet-installed  #-- Run cargo-vet supply chain audit
	cargo vet

.PHONY: check-audit-installed
check-audit-installed:
	@if ! cargo audit --version >/dev/null 2>&1; then \
		echo "cargo-audit is not installed. Install with 'cargo install cargo-audit'"; \
		exit 1; \
	fi

.PHONY: check-deny-installed
check-deny-installed:
	@if ! cargo deny --version >/dev/null 2>&1; then \
		echo "cargo-deny is not installed. Install with 'cargo install cargo-deny'"; \
		exit 1; \
	fi

.PHONY: check-edit-installed
check-edit-installed:
	@if ! cargo upgrade --version >/dev/null 2>&1; then \
		echo "cargo-edit is not installed. Install with 'cargo install cargo-edit'"; \
		exit 1; \
	fi

.PHONY: check-vet-installed
check-vet-installed:
	@if ! cargo vet --version >/dev/null 2>&1; then \
		echo "cargo-vet is not installed. Install with 'cargo install cargo-vet'"; \
		exit 1; \
	fi

#== Internal

.PHONY: help
help:  #-- Show this help message and exit
	@printf "Nautilus Agents Makefile\n\n"
	@printf "$(GREEN)Usage:$(RESET) make $(CYAN)<target>$(RESET)\n\n"
	@printf "$(GRAY)Tips: Use $(CYAN)make <target> V=1$(GRAY) for verbose output$(RESET)\n\n"
	@awk '\
	BEGIN { \
		FS = ":.*#--"; \
		target_maxlen = 0; \
		GREEN = "\033[0;32m"; \
		CYAN = "\033[0;36m"; \
		RESET = "\033[0m"; \
	} \
	/^[$$()% a-zA-Z_-]+:.*?#--/ { \
		if (length($$1) > target_maxlen) target_maxlen = length($$1); \
		targets[NR] = $$1; descriptions[NR] = $$2; \
	} \
	/^#==/ { \
		groups[NR] = substr($$0, 5); \
	} \
	END { \
		for (i = 1; i <= NR; i++) { \
			if (groups[i]) { \
				printf "\n" GREEN "%s:" RESET "\n", groups[i]; \
			} else if (targets[i]) { \
				printf "  " CYAN "%-*s" RESET " %s\n", target_maxlen, targets[i], descriptions[i]; \
			} \
		} \
	}' $(MAKEFILE_LIST)
