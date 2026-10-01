# 配布用のイメージ。リリースビルドしたバイナリだけを含む。
#   docker build -t pgsqlfmt .
#   docker run --rm -v "$PWD:/src" pgsqlfmt --check .
# pre-commit の `language: docker` のフック（.pre-commit-hooks.yaml）もこのイメージで動く。
# 開発用の環境は docker/dev/Dockerfile と compose.yaml を使う。
FROM rust:1-trixie AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:trixie-slim
COPY --from=build /build/target/release/pgsqlfmt /usr/local/bin/pgsqlfmt
WORKDIR /src
ENTRYPOINT ["pgsqlfmt"]
