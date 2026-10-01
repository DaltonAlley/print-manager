default:
    @just --list

dev:
    cargo leptos watch

check:
    cargo fmt --check
    cargo clippy --all-targets --all-features -- -D warnings
    just test

fmt:
    cargo fmt

lint:
    cargo clippy --all-targets --all-features -- -D warnings

test:
    cargo nextest run --all-features

build:
    cargo leptos build

release:
    cargo leptos build --release

clean:
    cargo clean
