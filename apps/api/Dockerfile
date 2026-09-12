FROM rust:1.97-bookworm AS builder

WORKDIR /app
COPY apps/api/Cargo.toml apps/api/Cargo.lock* ./
COPY apps/api/src ./src
COPY apps/api/migrations ./migrations
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates \
    && groupadd --system --gid 10001 knotree \
    && useradd --system --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin knotree \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/knotree-api /usr/local/bin/knotree-api

USER knotree

EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/knotree-api"]
