# ndl workspace helpers. Run `make` (or `make help`) for the target list.
# Targets document themselves with a trailing `## comment`.

CARGO ?= cargo

.DEFAULT_GOAL := help
.PHONY: help ndl ndld all chk test clean

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-zA-Z_-]+:.*## / { printf "  \033[36m%-8s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

ndl: ## Run the TUI (pass CLI args with ARGS="login bluesky")
	$(CARGO) run -p ndl -- $(ARGS)

# Dev mode: plain HTTP on localhost with placeholder credentials. Any of these
# already set in your environment win; ACME/TLS vars are always ignored here.
ndld: ## Run the OAuth server locally on http://localhost:8080 (dev placeholders)
	@echo "ndld dev server -> http://localhost:$${NDLD_PORT:-8080}  (Ctrl+C to stop)"
	env -u NDLD_ACME_DOMAIN -u NDLD_ACME_EMAIL -u NDLD_TLS_CERT -u NDLD_TLS_KEY \
	  NDLD_PORT="$${NDLD_PORT:-8080}" \
	  NDL_CLIENT_ID="$${NDL_CLIENT_ID:-dev-client-id}" \
	  NDL_CLIENT_SECRET="$${NDL_CLIENT_SECRET:-dev-client-secret}" \
	  NDLD_PUBLIC_URL="$${NDLD_PUBLIC_URL:-http://localhost:$${NDLD_PORT:-8080}}" \
	  RUST_LOG="$${RUST_LOG:-ndld=debug,info}" \
	  $(CARGO) run -p ndld

all: ## Build both binaries (debug)
	$(CARGO) build --workspace

chk: ## Pre-commit checklist: fmt, clippy (warnings are errors), check
	$(CARGO) fmt --all
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	$(CARGO) check --workspace

test: ## Run the whole test suite
	$(CARGO) test --workspace

clean: ## Remove build artifacts
	$(CARGO) clean
