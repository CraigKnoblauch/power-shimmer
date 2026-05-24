.PHONY: check lint test fmt build run

check:
	./scripts/check.sh

lint:
	./scripts/lint.sh

test:
	./scripts/test.sh

fmt:
	cargo fmt --all

build:
	cargo build --workspace

run:
	cargo run -p power-shimmer-app
