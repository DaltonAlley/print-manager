# Print Manager operations

## Run one container

Copy `.env.example` to an environment file kept outside source control, then create the configured data directory for the image's unprivileged user:

```sh
install -d -m 0700 "${PRINT_MANAGER_DATA_DIR:-./data}"
sudo chown 65532:65532 "${PRINT_MANAGER_DATA_DIR:-./data}"
docker compose up -d
```

The equivalent Podman Compose commands also work. The compose file has exactly one service and one mounted data directory at `/data`; the image needs no Node.js, browser, PostgreSQL, Redis, or PDF service.

`/data` is the indivisible persistent unit: it contains SQLite plus `attachments/` production files and `branding/` managed logos. `ATTACHMENT_MAX_BYTES` controls the per-file upload limit and defaults to `104857600` (100 MiB). Keep `PRINT_MANAGER_DATA_ROOT=/data` in the container. Startup performs a read-only attachment integrity audit and logs missing metadata targets, orphan files, and interrupted staging artifacts without deleting anything.

`SHOP_TIME_ZONE` controls calendar-day behavior such as “Today,” default due dates, and document issue dates. It accepts an IANA timezone name and defaults to `America/New_York`; stored timestamps remain UTC.

The published port defaults to `127.0.0.1:3000`. Do not set `PRINT_MANAGER_HOST=0.0.0.0` unless access is restricted to a trusted private network or an authenticated TLS reverse proxy. The app has no built-in authentication.

Set both `RESEND_API_KEY` and `RESEND_FROM` to enable email. The key is read only from the process environment. The Settings page reports **Configured** only when the running process accepted both values.

## Mandatory upgrade procedure

Every new Print Manager image runs pending SQLite migrations during startup. Before changing the pinned image tag, pulling a new image, or replacing the Compose file, complete these steps in order. Do not start the new image against live data until the restore rehearsal succeeds.

First stop the old image and copy the complete `/data` tree. The directory copy includes `print-manager.db` plus `print-manager.db-wal` and `print-manager.db-shm` when those files are present, all attachment bytes, and managed branding; copying the database file alone is not an upgrade backup.

```sh
docker compose stop app
data_dir="${PRINT_MANAGER_DATA_DIR:-./data}"
upgrade_backup="print-manager-pre-upgrade-$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$upgrade_backup"
cp -a "$data_dir/." "$upgrade_backup/"
test -f "$upgrade_backup/print-manager.db"
test -d "$upgrade_backup/attachments"
test -d "$upgrade_backup/branding"
(cd "$upgrade_backup" && find . -type f -print0 | sort -z | xargs -0 sha256sum) >"$upgrade_backup.sha256"
(cd "$upgrade_backup" && sha256sum --check "../$(basename "$upgrade_backup").sha256")
```

Next rehearse that backup with the still-pinned old image and an isolated restore directory. The live app remains stopped throughout the rehearsal.

```sh
upgrade_restore=./pre-upgrade-restore-rehearsal
mkdir -p "$upgrade_restore"
cp -a "$upgrade_backup/." "$upgrade_restore/"
sudo chown -R 65532:65532 "$upgrade_restore"
COMPOSE_PROJECT_NAME=print-manager-pre-upgrade-restore PRINT_MANAGER_DATA_DIR="$upgrade_restore" docker compose up -d app
curl --fail --silent --show-error http://127.0.0.1:3000/settings >/dev/null
COMPOSE_PROJECT_NAME=print-manager-pre-upgrade-restore PRINT_MANAGER_DATA_DIR="$upgrade_restore" docker compose down
```

Only after the checksum and old-image restore rehearsal pass, install the new release's Compose file or image tag and start it with `docker compose up -d app`. That startup is allowed to run migrations against the original data directory. Verify `/settings` and representative saved documents, then retain the pre-upgrade backup off-host until the upgraded app has been accepted. If migration or verification fails, stop the new image and follow the real restore procedure below; never start the old binary against a database already migrated by a newer version.

## Stop-and-copy backup

SQLite may have live WAL files, so stop the app before copying the entire data tree. Never back up only `print-manager.db`; attachment and branding bytes are required for a usable restore:

```sh
docker compose stop app
backup="print-manager-backup-$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$backup"
cp -a "${PRINT_MANAGER_DATA_DIR:-./data}/." "$backup/"
docker compose start app
```

Record a checksum and store the backup away from the host:

```sh
find "$backup" -type f -exec sha256sum {} \; >"$backup.sha256"
```

## Restore rehearsal

Always rehearse into a separate directory before relying on a backup:

```sh
restore_dir=./restore-rehearsal
mkdir -p "$restore_dir"
cp -a "$backup/." "$restore_dir/"
sudo chown -R 65532:65532 "$restore_dir"
docker compose stop app
COMPOSE_PROJECT_NAME=print-manager-restore PRINT_MANAGER_DATA_DIR="$restore_dir" docker compose up -d app
curl --fail --silent --show-error http://127.0.0.1:3000/settings >/dev/null
COMPOSE_PROJECT_NAME=print-manager-restore PRINT_MANAGER_DATA_DIR="$restore_dir" docker compose down
docker compose start app
```

For a real restore, stop the app, move the current data directory aside, copy the complete backup into a new empty data directory, preserve ownership for UID/GID `65532`, and start the app. Never overwrite a running SQLite database.

Run `nu --no-config-file scripts/operations-smoke.nu` to check the one-service/private-binding packaging and rehearse the copy/restore invariant locally. Run `docker compose build app` for a full image build.
