FROM --platform=$BUILDPLATFORM rust:slim-bookworm AS builder

ARG TARGETPLATFORM
ARG BUILDPLATFORM

WORKDIR /build

# Install musl toolchain for static builds
RUN apt-get update && apt-get install -y \
    musl-tools \
    && rm -rf /var/lib/apt/lists/*

# Add appropriate musl target based on platform
RUN case "$TARGETPLATFORM" in \
      "linux/amd64") echo "x86_64-unknown-linux-musl" > /tmp/target ;; \
      "linux/arm64") echo "aarch64-unknown-linux-musl" > /tmp/target ;; \
      *) echo "x86_64-unknown-linux-musl" > /tmp/target ;; \
    esac && \
    rustup target add $(cat /tmp/target)

# For cross-compiling to arm64, we need the linker
RUN if [ "$TARGETPLATFORM" = "linux/arm64" ] && [ "$BUILDPLATFORM" = "linux/amd64" ]; then \
      apt-get update && apt-get install -y gcc-aarch64-linux-gnu && rm -rf /var/lib/apt/lists/*; \
    fi

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY ndl ndl
COPY ndld ndld

# Build static release binary
RUN TARGET=$(cat /tmp/target) && \
    if [ "$TARGET" = "aarch64-unknown-linux-musl" ]; then \
      export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-gnu-gcc; \
    fi && \
    cargo build --release --target $TARGET -p ndld && \
    cp /build/target/$TARGET/release/ndld /build/ndld

# Runtime stage - distroless for minimal attack surface (~2MB base)
FROM gcr.io/distroless/static-debian12:nonroot

# Copy static binary
COPY --from=builder /build/ndld /ndld

ENV NDLD_PORT=8080
EXPOSE 8080

# Run as non-root (UID 10001)
USER 10001:10001

ENTRYPOINT ["/ndld"]
