SHELL := /bin/sh

.PHONY: help build release test lint fmt check install uninstall clean

help:
	@printf '%s\n' \
	  'make build     debug build' \
	  'make release   optimized build' \
	  'make test      run the test suite' \
	  'make lint      clippy, warnings as errors' \
	  'make fmt       format the workspace' \
	  'make check     fmt --check + lint + test' \
	  'make install   cargo install the CLI to ~/.cargo/bin' \
	  'make uninstall remove it again' \
	  'make clean     remove target/'

build:
	cargo build

release:
	cargo build --release

test:
	cargo test

lint:
	cargo clippy --all-targets -- -D warnings

fmt:
	cargo fmt --all

check:
	cargo fmt --all -- --check
	cargo clippy --all-targets -- -D warnings
	cargo test

install:
	cargo install --path crates/git-check --locked

uninstall:
	cargo uninstall git-check

clean:
	cargo clean
