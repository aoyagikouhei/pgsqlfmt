FROM rust:1-trixie

ARG USER_NAME=dev
ARG USER_UID=1000
ARG USER_GID=1000

# psql / pg_dump などのクライアントツール（検証用）
RUN apt-get update \
    && apt-get install -y --no-install-recommends postgresql-client \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt clippy

# スナップショットテスト（insta）の差分確認・承認用
RUN cargo install --locked cargo-insta \
    && rm -rf ${CARGO_HOME}/registry ${CARGO_HOME}/git

# ホストのユーザーと UID/GID を合わせ、生成物が root 所有にならないようにする
RUN groupadd --gid ${USER_GID} ${USER_NAME} \
    && useradd --uid ${USER_UID} --gid ${USER_GID} --create-home --shell /bin/bash ${USER_NAME}

# named volume のマウント先を先に作っておき、所有者を開発ユーザーにする
ENV CARGO_TARGET_DIR=/workspace-target
RUN mkdir -p /workspace ${CARGO_TARGET_DIR} ${CARGO_HOME}/registry ${CARGO_HOME}/git \
    && chown -R ${USER_UID}:${USER_GID} /workspace ${CARGO_TARGET_DIR} ${CARGO_HOME}

USER ${USER_NAME}
WORKDIR /workspace

CMD ["sleep", "infinity"]
