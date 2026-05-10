FROM rust:1.82-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p openvault-cli

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/vault /usr/local/bin/vault
EXPOSE 8080
CMD ["vault", "serve"]
