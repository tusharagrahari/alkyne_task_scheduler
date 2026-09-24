# Build with the toolchain the project is developed against.
FROM rust:1.89-slim-bookworm AS builder

WORKDIR /app

# Cache the dependency build: with only the manifests present, this layer is
# rebuilt solely when Cargo.toml / Cargo.lock change.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
    && echo 'fn main() {}' > src/main.rs \
    && echo '' > src/lib.rs \
    && cargo build --release --locked \
    && rm -rf src

COPY src ./src
COPY migrations ./migrations
# Touch the real sources so cargo does not reuse the placeholder build artefacts.
RUN touch src/main.rs src/lib.rs && cargo build --release --locked

FROM debian:bookworm-slim AS runtime

# TLS roots only: the binary links rustls, so no libssl is needed.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Run unprivileged.
RUN useradd --system --create-home --uid 10001 appuser
USER appuser

COPY --from=builder /app/target/release/alkyne /usr/local/bin/alkyne

EXPOSE 8080
CMD ["alkyne"]
