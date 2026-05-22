default: t

t *args:
    cargo build-sbf
    cargo test --workspace --features amm/test-helpers {{args}}

tt *args:
    cargo build-sbf
    cargo test --workspace --features amm/test-helpers {{args}} -- --nocapture --test-threads=1

# Run the lock/unlock PoC with full structured-log output. Designed for
# classroom use: the captured trees are the teaching artifact for
# docs/security/exercises/001-what-is-going-on.md.
poc:
    cargo build-sbf
    cargo test --workspace --features amm/test-helpers --test test_lock_unlock_attack -- --nocapture --test-threads=1

# Build and open documentation for amm and amm-math crates (no external deps)
doc:
    cargo doc --open --no-deps -p amm -p amm-math
