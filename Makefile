# PgPanel Makefile

CARGO ?= cargo
NPM ?= npm
TARGET_DIR ?= target
DIST_DIR ?= dist
VERSION := $(shell grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
ARCH := $(shell uname -m | sed 's/x86_64/amd64/;s/aarch64/arm64/')

.PHONY: all build test fmt clippy release frontend install-dev clean audit deny

all: build

build: frontend
	$(CARGO) build --release

test:
	$(CARGO) test --workspace

fmt:
	$(CARGO) fmt --all
	@$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

frontend:
	./scripts/build-frontend.sh

release: frontend
	$(CARGO) build --release
	mkdir -p $(DIST_DIR)/pgpanel-$(VERSION)/bin
	cp $(TARGET_DIR)/release/pgpanel-web $(DIST_DIR)/pgpanel-$(VERSION)/bin/
	cp $(TARGET_DIR)/release/pgpanel-helper $(DIST_DIR)/pgpanel-$(VERSION)/bin/
	cp $(TARGET_DIR)/release/pgpanel-updater $(DIST_DIR)/pgpanel-$(VERSION)/bin/
	cp -r static templates migrations $(DIST_DIR)/pgpanel-$(VERSION)/ 2>/dev/null || true
	echo "$(VERSION)" > $(DIST_DIR)/pgpanel-$(VERSION)/VERSION
	tar -czf $(DIST_DIR)/pgpanel-linux-$(ARCH).tar.gz -C $(DIST_DIR) pgpanel-$(VERSION)
	@echo "Release archive: $(DIST_DIR)/pgpanel-linux-$(ARCH).tar.gz"

install-dev:
	$(CARGO) build
	$(NPM) ci
	$(MAKE) frontend
	@echo "Development build complete. Run with:"
	@echo "  PGPANEL_CONFIG=./config/pgpanel.toml ./target/debug/pgpanel-helper --dev &"
	@echo "  PGPANEL_CONFIG=./config/pgpanel.toml ./target/debug/pgpanel-web --dev"

audit:
	$(CARGO) audit

deny:
	$(CARGO) deny check

clean:
	$(CARGO) clean
	rm -rf node_modules $(DIST_DIR) static/css/app.css
