# matou is a kitty remote-control TUI: it must run *inside* kitty (it needs $KITTY_LISTEN_ON to
# talk to `kitty @`), so a container can't run it usefully. This image just builds the binary
# reproducibly — for CI or a release artifact.
#
# Build + extract the binary to ./matou (BuildKit):
#   docker build --output . .
ARG RUST_VERSION=1.90

FROM rust:${RUST_VERSION} AS build
WORKDIR /src
COPY . .
RUN cargo build --release && cp target/release/matou /matou

# final stage = just the binary, so `docker build --output .` writes ./matou to the host
FROM scratch
COPY --from=build /matou /matou
