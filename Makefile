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
	pre-commit run --all-files

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

#== Testing

.PHONY: cargo-test
cargo-test: export RUST_BACKTRACE=1
cargo-test:  #-- Run all Rust tests
	$(info $(M) Running Rust tests...)
	cargo test

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

.PHONY: security-audit
security-audit: check-audit-installed check-deny-installed check-vet-installed  #-- Run full security audit
	$(info $(M) Running security audit...)
	@cargo audit --color never
	@cargo deny --all-features check advisories licenses sources bans
	@cargo vet --locked
	@printf "$(GREEN)Security audit passed$(RESET)\n"

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
