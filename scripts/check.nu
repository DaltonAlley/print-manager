#!/usr/bin/env nu

const project_root = path self | path dirname | path join .. | path expand

def run-command [project_dir: path command: closure] {
    let result = do { cd $project_dir; do $command } | complete
    if not ($result.stdout | is-empty) { print --no-newline $result.stdout }
    if not ($result.stderr | is-empty) { print --stderr --no-newline $result.stderr }
    $result.exit_code
}

def main [project: string = "print-manager"] {
    if $project != "print-manager" {
        print --stderr $"Unknown project: ($project)"
        exit 2
    }
    for executable in [cargo cargo-nextest] {
        if (which $executable | is-empty) {
            print --stderr $"Missing required command: ($executable)"
            exit 127
        }
    }
    let commands = [
        {|| ^cargo fmt --all --check }
        {|| ^cargo clippy --locked --all-targets --all-features -- -D warnings }
        {|| ^cargo nextest run --all-features --locked }
        {|| ^nu --no-config-file scripts/operations-smoke.nu }
    ]
    for command in $commands {
        let status = run-command $project_root $command
        if $status != 0 { exit $status }
    }
    print "All requested checks passed."
}
