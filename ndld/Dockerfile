FROM rust:1.83-slim-bookworm AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace files
COPY Cargo.toml ./
COPY ndl ndl
COPY ndld ndld

# Build release binary
RUN cargo build --release -p ndld

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/ndld /usr/local/bin/ndld

ENV NDLD_PORT=8080
EXPOSE 8080

CMD ["ndld"]
