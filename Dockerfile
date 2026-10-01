# 配布用のイメージ。リリースビルドしたバイナリだけを含む。
#   docker build -t sql-formatter .
#   docker run --rm -v "$PWD:/src" sql-formatter --check .
# pre-commit の `language: docker` のフック（.pre-commit-hooks.yaml）もこのイメージで動く。
# 開発用の環境は docker/dev/Dockerfile と compose.yaml を使う。
FROM rust:1-trixie AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:trixie-slim
COPY --from=build /build/target/release/sql-formatter /usr/local/bin/sql-formatter
WORKDIR /src
ENTRYPOINT ["sql-formatter"]
