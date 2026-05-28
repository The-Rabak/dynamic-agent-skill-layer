# syntax=docker/dockerfile:1.7

FROM lukemathwalker/cargo-chef:latest-rust-1 AS planner
WORKDIR /app
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM lukemathwalker/cargo-chef:latest-rust-1 AS builder
WORKDIR /app
RUN rustup target add x86_64-unknown-linux-musl
COPY --from=planner /app/recipe.json recipe.json
ENV SQLX_OFFLINE=true
RUN --mount=type=cache,target=/app/target \
    cargo chef cook --release --recipe-path recipe.json --target x86_64-unknown-linux-musl
COPY . .
ARG BIN
RUN --mount=type=cache,target=/app/target \
    cargo build --release --target x86_64-unknown-linux-musl --bin ${BIN}

FROM alpine:3.21 AS runtime
ARG BIN
ENV RUST_LOG=info
RUN apk add --no-cache ca-certificates wget
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/${BIN} /usr/local/bin/${BIN}
RUN ln -s /usr/local/bin/${BIN} /usr/local/bin/service-bin
ENTRYPOINT ["/usr/local/bin/service-bin"]
