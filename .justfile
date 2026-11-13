@_default:
    just --list

[doc('Set the repository up: the hooks')]
@install:
    -prek install --hook-type pre-commit --hook-type commit-msg

[doc('Run the test suite')]
@test *args:
    cargo test --locked {{ args }}

[doc('Lint and check formatting')]
@lint:
    cargo fmt --check
    cargo clippy --locked --all-targets -- --deny warnings

[doc('Build the release binary')]
@build:
    cargo build --release

[doc('Put this build of freightscan on PATH')]
@deploy:
    cargo install --path "{{ justfile_directory() }}" --locked
