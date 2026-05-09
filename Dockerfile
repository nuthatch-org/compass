FROM rust:1.85-slim AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock* ./
COPY crates/ crates/

RUN cargo build --release --bin compass-gateway

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/compass-gateway /app/compass-gateway

EXPOSE 8090

CMD ["/app/compass-gateway"]
