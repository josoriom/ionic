CARGO_BIN         := ionic
CLI_MANIFEST      := Cargo.toml

WORKSPACE_ROOT    := $(abspath $(CURDIR)/../..)
WORKSPACE_TARGET  := $(WORKSPACE_ROOT)/target
ARTIFACTS         := $(WORKSPACE_ROOT)/artifacts
RELEASE_DIR       := $(ARTIFACTS)/$(PACKAGE_VERSION)

DOCKER_IMAGE	  := rust:1-bookworm

WINDOWS_ARM64_IMAGE  := dockcross/windows-arm64:latest
WINDOWS_ARM64_TARGET := aarch64-pc-windows-gnullvm

define guard
	@test -n "$(PACKAGE_VERSION)" || { echo "PACKAGE_VERSION is not set; run 'make release' from the repo root"; exit 1; }
	@test ! -e "$(1)" || { echo "artifact already exists: $(1) (bump version in config.json or run 'make clean')"; exit 1; }
endef

.PHONY: all mac linux windows macos-arm64 macos-x86_64 linux-amd64 linux-arm64 windows-amd64 windows-arm64 clean

all: mac linux windows

mac: macos-arm64 macos-x86_64
linux: linux-amd64 linux-arm64
windows: windows-amd64

macos-arm64:
	$(call guard,$(RELEASE_DIR)/aarch64-apple-darwin/$(CARGO_BIN))
	rustup target add aarch64-apple-darwin
	cargo build --manifest-path $(CLI_MANIFEST) --release --locked --target aarch64-apple-darwin
	mkdir -p $(RELEASE_DIR)/aarch64-apple-darwin
	cp $(WORKSPACE_TARGET)/aarch64-apple-darwin/release/$(CARGO_BIN) $(RELEASE_DIR)/aarch64-apple-darwin/$(CARGO_BIN)

macos-x86_64:
	$(call guard,$(RELEASE_DIR)/x86_64-apple-darwin/$(CARGO_BIN))
	rustup target add x86_64-apple-darwin
	cargo build --manifest-path $(CLI_MANIFEST) --release --locked --target x86_64-apple-darwin
	mkdir -p $(RELEASE_DIR)/x86_64-apple-darwin
	cp $(WORKSPACE_TARGET)/x86_64-apple-darwin/release/$(CARGO_BIN) $(RELEASE_DIR)/x86_64-apple-darwin/$(CARGO_BIN)

linux-amd64:
	$(call guard,$(RELEASE_DIR)/x86_64-unknown-linux-gnu/$(CARGO_BIN))
	docker run --rm --platform=linux/amd64 \
	  -e CARGO_TARGET_DIR=/work/target-linux-amd64 \
	  -v $$PWD/../..:/work -w /work/crates/cli \
	  --entrypoint /usr/local/cargo/bin/cargo $(DOCKER_IMAGE) \
	  build --manifest-path Cargo.toml --release --locked
	mkdir -p $(RELEASE_DIR)/x86_64-unknown-linux-gnu
	cp $(WORKSPACE_ROOT)/target-linux-amd64/release/$(CARGO_BIN) $(RELEASE_DIR)/x86_64-unknown-linux-gnu/$(CARGO_BIN)

linux-arm64:
	$(call guard,$(RELEASE_DIR)/aarch64-unknown-linux-gnu/$(CARGO_BIN))
	docker run --rm --platform=linux/arm64 \
	  -e CARGO_TARGET_DIR=/work/target-linux-arm64 \
	  -v $$PWD/../..:/work -w /work/crates/cli \
	  --entrypoint /usr/local/cargo/bin/cargo $(DOCKER_IMAGE) \
	  build --manifest-path Cargo.toml --release --locked
	mkdir -p $(RELEASE_DIR)/aarch64-unknown-linux-gnu
	cp $(WORKSPACE_ROOT)/target-linux-arm64/release/$(CARGO_BIN) $(RELEASE_DIR)/aarch64-unknown-linux-gnu/$(CARGO_BIN)

windows-amd64:
	$(call guard,$(RELEASE_DIR)/x86_64-pc-windows-gnu/$(CARGO_BIN).exe)
	docker run --rm --platform=linux/amd64 \
	  -e CARGO_TARGET_DIR=/work/target-windows-amd64 \
	  -v $$PWD/../..:/work -w /work/crates/cli $(DOCKER_IMAGE) bash -lc '\
	    set -euo pipefail; \
	    export PATH=/usr/local/cargo/bin:$$PATH; \
	    apt-get update && apt-get install -y --no-install-recommends \
	      curl ca-certificates gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 mingw-w64; \
	    command -v rustup >/dev/null 2>&1 || { curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal; . "$$HOME/.cargo/env"; }; \
	    rustup target add x86_64-pc-windows-gnu; \
	    RUSTFLAGS="-C linker=x86_64-w64-mingw32-gcc" \
	      cargo build --manifest-path Cargo.toml --release --locked --target x86_64-pc-windows-gnu'
	mkdir -p $(RELEASE_DIR)/x86_64-pc-windows-gnu
	cp $(WORKSPACE_ROOT)/target-windows-amd64/x86_64-pc-windows-gnu/release/$(CARGO_BIN).exe $(RELEASE_DIR)/x86_64-pc-windows-gnu/$(CARGO_BIN).exe

windows-arm64:
	$(call guard,$(RELEASE_DIR)/$(WINDOWS_ARM64_TARGET)/$(CARGO_BIN).exe)
	docker run --rm --platform=linux/amd64 \
	  -e CARGO_TARGET_DIR=/work/target-windows-arm64 \
	  -v $$PWD/../..:/work -w /work/crates/cli $(WINDOWS_ARM64_IMAGE) \
	  -- bash -lc 'set -euo pipefail; \
	    rustup target add $(WINDOWS_ARM64_TARGET); \
	    export CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_LINKER="$$CC"; \
	    cargo build --manifest-path Cargo.toml --release --locked --target $(WINDOWS_ARM64_TARGET)'
	mkdir -p $(RELEASE_DIR)/$(WINDOWS_ARM64_TARGET)
	cp $(WORKSPACE_ROOT)/target-windows-arm64/$(WINDOWS_ARM64_TARGET)/release/$(CARGO_BIN).exe $(RELEASE_DIR)/$(WINDOWS_ARM64_TARGET)/$(CARGO_BIN).exe

clean:
	cargo clean --manifest-path $(CLI_MANIFEST)
	rm -rf $(ARTIFACTS) \
	  $(WORKSPACE_ROOT)/target-linux-amd64 \
	  $(WORKSPACE_ROOT)/target-linux-arm64 \
	  $(WORKSPACE_ROOT)/target-windows-amd64 \
	  $(WORKSPACE_ROOT)/target-windows-arm64
