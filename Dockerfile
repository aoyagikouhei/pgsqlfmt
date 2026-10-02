# Distribution image. Contains only the release-built binary.
#   docker build -t pgsqlfmt .
#   docker run --rm -v "$PWD:/src" pgsqlfmt --check .
# The pre-commit `language: docker` hooks (.pre-commit-hooks.yaml) also run on this image.
# The development environment uses docker/dev/Dockerfile and compose.yaml.
FROM rust:1-trixie AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:trixie-slim
COPY --from=build /build/target/release/pgsqlfmt /usr/local/bin/pgsqlfmt
WORKDIR /src
ENTRYPOINT ["pgsqlfmt"]
