FROM rust:1-trixie

ARG USER_NAME=dev
ARG USER_UID=1000
ARG USER_GID=1000

# Client tools such as psql / pg_dump (for verification)
RUN apt-get update \
    && apt-get install -y --no-install-recommends postgresql-client \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt clippy

# For reviewing and accepting snapshot test (insta) diffs
RUN cargo install --locked cargo-insta \
    && rm -rf ${CARGO_HOME}/registry ${CARGO_HOME}/git

# Match the host user's UID/GID so that build artifacts are not owned by root
RUN groupadd --gid ${USER_GID} ${USER_NAME} \
    && useradd --uid ${USER_UID} --gid ${USER_GID} --create-home --shell /bin/bash ${USER_NAME}

# Create the named volume mount points in advance and make the dev user their owner
ENV CARGO_TARGET_DIR=/workspace-target
RUN mkdir -p /workspace ${CARGO_TARGET_DIR} ${CARGO_HOME}/registry ${CARGO_HOME}/git \
    && chown -R ${USER_UID}:${USER_GID} /workspace ${CARGO_TARGET_DIR} ${CARGO_HOME}

USER ${USER_NAME}
WORKDIR /workspace

CMD ["sleep", "infinity"]
