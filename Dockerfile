FROM rust:1.87-slim-bookworm AS chef
# We only pay the installation cost once, 
# it will be cached from the second build onwards
RUN apt-get update -y && apt-get -y install pkg-config libssl-dev libpq-dev g++ curl protobuf-compiler
RUN cargo install cargo-chef 
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare  --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --recipe-path recipe.json --bin "vault"
# Build application
COPY . .
RUN cargo build --bin "vault"

FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update -y; \
    apt-get install -y \
    pkg-config \
    build-essential\
    libssl-dev \
    ca-certificates \
    curl \
    protobuf-compiler \
    unzip \
    ;

COPY --from=builder /app/target/debug/vault /app/vault

EXPOSE 8090
ENTRYPOINT ["/app/vault"]
