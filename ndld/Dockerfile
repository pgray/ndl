FROM rust:slim-bookworm AS builder

WORKDIR /build

# Install musl toolchain for static builds
RUN apt-get update && apt-get install -y \
    musl-tools \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add x86_64-unknown-linux-musl

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY ndl ndl
COPY ndld ndld

# Build static release binary
RUN cargo build --release --target x86_64-unknown-linux-musl -p ndld

# Runtime stage - distroless for minimal attack surface (~2MB base)
FROM gcr.io/distroless/static-debian12:nonroot

COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/ndld /ndld

ENV NDLD_PORT=8080
EXPOSE 8080

# Run as non-root (UID 10001)
USER 10001:10001

ENTRYPOINT ["/ndld"]
