#!/usr/bin/env nu

const project_dir = path self ..

def fail [message: string] {
    error make { msg: $"print-manager operations smoke: ($message)" }
}

def assert-count [expected: int, pattern: string, content: string, file: path] {
    let actual = $content | lines | where {|line| $line | str contains $pattern } | length
    if $actual != $expected {
        fail $"expected ($expected) occurrence(s) of <($pattern)> in ($file), got ($actual)"
    }
}

def require-text [content: string, pattern: string, file: path] {
    if not ($content | str contains $pattern) {
        fail $"expected <($pattern)> in ($file)"
    }
}

def copy-tree [source: path, destination: path] {
    let result = do { ^cp -a $"($source)/." $"($destination)/" } | complete
    if $result.exit_code != 0 {
        if not ($result.stderr | is-empty) { print --stderr --no-newline $result.stderr }
        fail $"could not copy ($source) to ($destination)"
    }
}

def main [] {
    let compose = $project_dir | path join docker-compose.yml
    let containerfile = $project_dir | path join Containerfile
    let operations = $project_dir | path join OPERATIONS.md
    let manifest = $project_dir | path join Cargo.toml
    let compose_text = open --raw $compose
    let containerfile_text = open --raw $containerfile
    let operations_text = open --raw $operations
    let manifest_text = open --raw $manifest

    let compose_lines = $compose_text | lines
    let services_index = $compose_lines
        | enumerate
        | where item == "services:"
        | first
        | get index
    let service_count = $compose_lines
        | skip ($services_index + 1)
        | take while {|line| ($line | is-empty) or ($line | str starts-with " ") }
        | where {|line| $line =~ '^  [a-zA-Z0-9_-]+:$' }
        | length
    if $service_count != 1 { fail $"expected one Compose service, got ($service_count)" }

    require-text $compose_text '${PRINT_MANAGER_HOST:-127.0.0.1}:3000:3000' $compose
    require-text $compose_text '${PRINT_MANAGER_DATA_DIR:-./data}:/data' $compose
    require-text $containerfile_text 'USER 65532:65532' $containerfile
    require-text $operations_text 'sudo chown -R 65532:65532 "$restore_dir"' $operations
    require-text $manifest_text 'version = "0.3.4"' $manifest

    let count_contracts = [
        [2 'FROM --platform=$BUILDPLATFORM docker.io/library/rust@sha256:7c4ae649a84014c467d79319bbf17ce2632ae8b8be123ac2fb2ea5be46823f31 AS']
        [1 'AS frontend']
        [1 'AS backend']
        [1 'ARG BUILDARCH']
        [1 'RUN test "$BUILDARCH" = amd64']
        [1 'ARG TARGETARCH']
        [1 'RUN test "$TARGETARCH" = amd64 -o "$TARGETARCH" = arm64']
        [0 'cargo install']
        [0 'cargo leptos build --server-only']
        [1 'cargo-leptos-x86_64-unknown-linux-gnu.tar.gz']
        [1 'cargo-leptos-x86_64-unknown-linux-gnu/cargo-leptos']
        [1 'fda80f4845e92d0e8f5ec13cf1a46982ba7a518ae01182e7e4201312944bc05d']
        [1 "test \"$(cargo leptos --version)\" = 'cargo-leptos 0.3.7'"]
        [1 'cargo leptos build --frontend-only --release --lib-cargo-args=--locked']
        [1 'cp -a target/site /out/site']
        [2 'target="$(if test "$TARGETARCH" = amd64; then printf x86_64-unknown-linux-gnu; else printf aarch64-unknown-linux-gnu; fi)"']
        [1 'rustup target add "$target"']
        [1 'gcc-aarch64-linux-gnu=4:12.2.0-3']
        [1 'libc6-dev-arm64-cross=2.36-8cross1']
        [1 'CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc']
        [1 'CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc']
        [2 'cargo build --locked --release --target "$target"']
        [1 'install -D -m 0755 "target/$target/release/print-manager" /out/print-manager']
        [1 'COPY --from=backend /out/print-manager /usr/local/bin/print-manager']
        [1 'COPY --from=frontend /out/site /app/site']
        [2 'https://snapshot.debian.org/archive/debian/20260801T000000Z bookworm main']
        [1 'http://snapshot.debian.org/archive/debian/20260801T000000Z bookworm main']
        [3 'apt-get -o Acquire::Check-Valid-Until=false update']
        [2 'ca-certificates=20230311+deb12u1']
        [1 'clang=1:14.0-55.7~deb12u1']
        [1 'curl=7.88.1-10+deb12u15']
        [1 'gcc-aarch64-linux-gnu=4:12.2.0-3']
        [1 'libc6-dev-arm64-cross=2.36-8cross1']
    ]
    for contract in $count_contracts {
        assert-count $contract.0 $contract.1 $containerfile_text $containerfile
    }

    for forbidden_input in [
        deb.debian.org
        security.debian.org
        /debian/current
        ':latest'
        '[trusted=yes]'
        Acquire::AllowInsecureRepositories
        '--allow-unauthenticated'
    ] {
        if ($containerfile_text | str contains $forbidden_input) {
            fail $"forbidden Containerfile input: ($forbidden_input)"
        }
    }

    let markers = [
        '## Mandatory upgrade procedure'
        'docker compose stop app'
        'print-manager.db-wal'
        'print-manager.db-shm'
        'all attachment bytes'
        'managed branding'
        'read-only attachment integrity audit'
        'sha256sum --check'
        'COMPOSE_PROJECT_NAME=print-manager-pre-upgrade-restore'
        'Only after the checksum and old-image restore rehearsal pass'
        'That startup is allowed to run migrations'
    ]
    for marker in $markers {
        require-text $operations_text $marker $operations
    }

    let heading_index = $operations_text | str index-of '## Mandatory upgrade procedure'
    let section_end = $operations_text | str index-of '## Stop-and-copy backup'
    let stop_index = $operations_text | str index-of 'docker compose stop app'
    let checksum_index = $operations_text | str index-of 'sha256sum --check'
    let restore_index = $operations_text | str index-of 'COMPOSE_PROJECT_NAME=print-manager-pre-upgrade-restore'
    let migrate_index = $operations_text | str index-of 'That startup is allowed to run migrations'
    if not (
        $heading_index < $stop_index
        and $stop_index < $checksum_index
        and $checksum_index < $restore_index
        and $restore_index < $migrate_index
        and $migrate_index < $section_end
    ) {
        fail "mandatory upgrade steps are not in the required order"
    }

    let smoke_dir = $nu.temp-dir | path join $"print-manager-operations-smoke-(random uuid)"
    let data_dir = $smoke_dir | path join data
    let backup_dir = $smoke_dir | path join backup
    let restore_dir = $smoke_dir | path join restore
    mkdir $data_dir $backup_dir $restore_dir
    mkdir ($data_dir | path join attachments .staging) ($data_dir | path join branding .staging)

    try {
        $"SQLite format 3(char nul)print-manager-operations-smoke" | save ($data_dir | path join print-manager.db)
        "wal-state" | save ($data_dir | path join print-manager.db-wal)
        "shared-memory-state" | save ($data_dir | path join print-manager.db-shm)
        "production-art" | save ($data_dir | path join attachments 00000000-0000-0000-0000-000000000001)
        "managed-logo" | save ($data_dir | path join branding 00000000-0000-0000-0000-000000000002)
        copy-tree $data_dir $backup_dir
        copy-tree $backup_dir $restore_dir

        for persistent_file in [
            print-manager.db
            print-manager.db-wal
            print-manager.db-shm
            attachments/00000000-0000-0000-0000-000000000001
            branding/00000000-0000-0000-0000-000000000002
        ] {
            let source = open --raw ($data_dir | path join $persistent_file)
            let restored = open --raw ($restore_dir | path join $persistent_file)
            if $source != $restored {
                fail $"restored file differs from source: ($persistent_file)"
            }
        }
    } catch {|error|
        rm --recursive --force $smoke_dir
        print --stderr $error.msg
        exit 1
    }
    rm --recursive --force $smoke_dir
    print "Print Manager packaging and stop-copy-restore invariants passed."
}
