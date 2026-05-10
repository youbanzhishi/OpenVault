# ============================================================================
# OpenVault Dockerfile — Multi-stage build
# ============================================================================
# Stage 1: Build the Rust binary
# Stage 2: Minimal runtime image
# ============================================================================

# ---------- Builder stage ----------
FROM rust:1.85-slim AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y     pkg-config     libssl-dev     && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/openvault

# Cache dependencies by copying manifests first
COPY Cargo.toml Cargo.lock ./
COPY crates/openvault-core/Cargo.toml crates/openvault-core/Cargo.toml
COPY crates/openvault-storage/Cargo.toml crates/openvault-storage/Cargo.toml
COPY crates/openvault-cli/Cargo.toml crates/openvault-cli/Cargo.toml
COPY crates/openvault-transport/Cargo.toml crates/openvault-transport/Cargo.toml
COPY crates/openvault-server/Cargo.toml crates/openvault-server/Cargo.toml
COPY crates/openvault-intel/Cargo.toml crates/openvault-intel/Cargo.toml

# Create dummy source files so cargo can resolve the workspace
RUN mkdir -p crates/openvault-core/src && echo "" > crates/openvault-core/src/lib.rs  && mkdir -p crates/openvault-storage/src && echo "" > crates/openvault-storage/src/lib.rs  && mkdir -p crates/openvault-cli/src && echo "fn main(){}" > crates/openvault-cli/src/main.rs  && mkdir -p crates/openvault-transport/src && echo "" > crates/openvault-transport/src/lib.rs  && mkdir -p crates/openvault-server/src && echo "fn main(){}" > crates/openvault-server/src/main.rs  && mkdir -p crates/openvault-intel/src && echo "" > crates/openvault-intel/src/lib.rs

# Build dependencies only (cached layer)
RUN cargo build --release -p openvault-server 2>/dev/null || true

# Now copy the real source code
COPY crates/openvault-core/src/ crates/openvault-core/src/
COPY crates/openvault-storage/src/ crates/openvault-storage/src/
COPY crates/openvault-cli/src/ crates/openvault-cli/src/
COPY crates/openvault-transport/src/ crates/openvault-transport/src/
COPY crates/openvault-server/src/ crates/openvault-server/src/
COPY crates/openvault-intel/src/ crates/openvault-intel/src/

# Touch the files to invalidate the cached build
RUN find crates -name "*.rs" -exec touch {} +

# Build the server binary
RUN cargo build --release -p openvault-server

# ---------- Runtime stage ----------
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y     ca-certificates     curl     tzdata     && rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN groupadd -r openvault && useradd -r -g openvault -m -d /home/openvault openvault

# Copy the binary from builder
COPY --from=builder /usr/src/openvault/target/release/openvault-server /usr/local/bin/openvault-server

# Create data directories
RUN mkdir -p /data/db /data/backups /data/logs     && chown -R openvault:openvault /data

# Default configuration
ENV RUST_LOG=openvault_server=info
ENV OPENVAULT_BIND=0.0.0.0:8090
ENV OPENVAULT_DB_PATH=/data/db/openvault.db
ENV OPENVAULT_BACKUP_PATH=/data/backups

# Expose the server port
EXPOSE 8090

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3     CMD curl -f http://localhost:8090/api/v1/health || exit 1

# Run as non-root user
USER openvault

# Volume for persistent data
VOLUME ["/data"]

ENTRYPOINT ["openvault-server"]
CMD ["--bind", "0.0.0.0:8090"]
