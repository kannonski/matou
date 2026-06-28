# matou is a kitty remote-control TUI: it must run *inside* kitty (it needs $KITTY_LISTEN_ON
# to talk to `kitty @`), so a container can't run it usefully. This image exists to build the
# static binary reproducibly — for CI or a release artifact.
#
# Build + extract the binary to ./matou (BuildKit):
#   docker build --output . .
# Or grab it from the image:
#   docker build -t matou . && id=$(docker create matou) && docker cp $id:/matou ./matou && docker rm $id

ARG GO_VERSION=1.26

FROM golang:${GO_VERSION}-alpine AS build
WORKDIR /src
# cache deps first
COPY go.mod go.sum ./
RUN go mod download
COPY . .
# fully static, stripped, reproducible (-trimpath drops local paths)
RUN CGO_ENABLED=0 go build -trimpath -ldflags='-s -w' -o /matou ./cmd/matou

# final stage = just the binary, so `docker build --output .` writes ./matou to the host
FROM scratch
COPY --from=build /matou /matou
