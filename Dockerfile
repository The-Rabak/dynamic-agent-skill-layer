# syntax=docker/dockerfile:1.7

FROM lukemathwalker/cargo-chef:latest-rust-1 AS planner
WORKDIR /app
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM lukemathwalker/cargo-chef:latest-rust-1 AS builder
WORKDIR /app
# musl-tools provides the musl C cross-compiler (musl-gcc) that ring/cc-rs need
# to build for the x86_64-unknown-linux-musl target. cc-rs derives the tool name
# `x86_64-linux-musl-gcc` from the triple; CC_<target> redirects it to musl-gcc.
RUN apt-get update \
    && apt-get install -y --no-install-recommends musl-tools musl-dev \
    && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-unknown-linux-musl
ENV CC_x86_64_unknown_linux_musl=musl-gcc
COPY --from=planner /app/recipe.json recipe.json
ENV SQLX_OFFLINE=true
RUN --mount=type=cache,target=/app/target \
    cargo chef cook --release --recipe-path recipe.json --target x86_64-unknown-linux-musl
COPY . .
ARG BIN
# `/app/target` is a BuildKit cache mount and is NOT persisted into the image
# layer, so the built binary must be copied OUT of the mount within the same RUN
# for the runtime stage's COPY --from=builder to find it.
RUN --mount=type=cache,target=/app/target \
    cargo build --release --target x86_64-unknown-linux-musl --bin ${BIN} \
    && cp /app/target/x86_64-unknown-linux-musl/release/${BIN} /app/service-bin

FROM alpine:3.21 AS runtime
ARG BIN
ENV RUST_LOG=info
RUN apk add --no-cache ca-certificates wget
COPY --from=builder /app/service-bin /usr/local/bin/${BIN}
RUN ln -s /usr/local/bin/${BIN} /usr/local/bin/service-bin
ENTRYPOINT ["/usr/local/bin/service-bin"]
