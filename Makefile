.PHONY: build run test lint clean

build:
	cargo build --release

run: build
	./target/release/calendar-cli

test:
	cargo test

lint:
	cargo fmt --check
	cargo clippy --release

clean:
	cargo clean
	rm -rf target
