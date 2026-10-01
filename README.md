# Print Manager

A self-hosted print job manager for print shops, built with Rust and Leptos. It manages production workflows, job documents, scheduling, shop settings, and customer contact information.

## Develop and build

From this repository's root, enter the pinned development environment:

```sh
nix develop .#print-manager
cargo leptos watch
```

Build production assets and the server with `cargo leptos build --release` (or `just release`). Configuration examples are in `.env.example`.

## Check

```sh
nix develop .#print-manager -c nu --no-config-file scripts/check.nu print-manager
```

This runs formatting, locked Clippy checks, nextest, and the operations smoke test. GitHub Actions runs the same checks on pushes to `main`, pull requests, and manual dispatches.

## Deploy

The included `Containerfile` and `docker-compose.yml` build and run the application with persistent data. With Docker Compose available:

```sh
docker compose up --build -d
```

The default host binding is `127.0.0.1:3000`. Set `PRINT_MANAGER_DATA_DIR` to the persistent data directory and configure the shop timezone and optional email settings as needed. See [OPERATIONS.md](OPERATIONS.md) for storage, backup, restore, and operational checks.
