set dotenv-load := false
set positional-arguments

default: check

run *args:
    cargo run --bin remarkable -- "$@"

run-ai *args:
    AGENT=1 cargo run --bin remarkable -- "$@"

fmt:
    cargo fmt --package remarkable-cli

test:
    cargo test --all-targets

lint:
    cargo clippy --all-targets -- -D warnings

coverage:
    cargo llvm-cov --all-targets --fail-under-lines 90

check:
    cargo fmt --package remarkable-cli -- --check
    cargo clippy --all-targets -- -D warnings
    cargo test --all-targets
    cargo llvm-cov --all-targets --fail-under-lines 90

build:
    cargo build --bin remarkable
    mkdir -p bin
    cp target/debug/remarkable bin/remarkable

# Manual cloud/tablet workflow; every write goes through bin/remarkable.
native-import-check *args: build
    bun install --no-env-file --cwd scripts/native-import-check --frozen-lockfile
    bun --no-env-file scripts/native-import-check/runCheck.ts "$@"

native-import-recheck *args: build
    bun install --no-env-file --cwd scripts/native-import-check --frozen-lockfile
    bun --no-env-file scripts/native-import-check/recheckPageIdentity.ts "$@"

native-import-typecheck:
    bun install --no-env-file --cwd scripts/native-import-check --frozen-lockfile
    bun run --no-env-file --cwd scripts/native-import-check typecheck

