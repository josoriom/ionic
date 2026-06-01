.PHONY: sync check show build test release

sync:
	cargo run -p xtask --quiet -- sync

check:
	cargo run -p xtask --quiet -- check

show:
	cargo run -p xtask --quiet -- show

build:
	cargo build --workspace

test:
	cargo test --workspace

release: sync
	$(MAKE) -C crates/cli all
