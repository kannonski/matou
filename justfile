# matou — task runner (https://github.com/casey/just). Run `just` for the default build.
prefix := env_var_or_default("PREFIX", env_var("HOME") / ".local")

# build (release)
build:
    cargo build --release

# build + install to $PREFIX/bin (default ~/.local/bin)
install:
    cargo build --release
    install -Dm755 target/release/matou {{ prefix }}/bin/matou

# build, then run with extra args (e.g. `just run -- --once`)
run *args:
    cargo run --release -- {{ args }}

# format · clippy · test — the pre-commit sweep
check: fmt clippy test

fmt:
    cargo fmt

clippy:
    cargo clippy --all-targets

test:
    cargo test

# build the binary in Docker and export it to ./matou (needs BuildKit)
docker:
    docker build --output . .

clean:
    cargo clean
