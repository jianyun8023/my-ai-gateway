ARG NODE_VERSION=24.11.1-bookworm
ARG RUST_VERSION=1.97.1-bookworm
ARG CARGO_CHEF_VERSION=0.1.78

FROM node:${NODE_VERSION} AS web-builder
WORKDIR /app/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web ./
RUN npm run build

FROM lukemathwalker/cargo-chef:${CARGO_CHEF_VERSION}-rust-${RUST_VERSION} AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Keep dependency compilation in a layer that is independent from application source.
RUN cargo chef cook --release --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY crates ./crates
COPY migrations ./migrations
COPY config.example.json ./config.example.json
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && groupadd --system --gid 10001 gateway \
    && useradd --system --uid 10001 --gid gateway --home-dir /nonexistent --no-create-home gateway \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /opt/my-ai-gateway
COPY --from=builder --chown=gateway:gateway /app/target/release/my-ai-gateway /usr/local/bin/my-ai-gateway
COPY --from=web-builder --chown=gateway:gateway /app/web/dist ./web/dist
EXPOSE 8787
ENV GATEWAY_LISTEN_ADDR=0.0.0.0:8787
USER gateway
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=5 \
    CMD ["curl", "-fsS", "http://127.0.0.1:8787/healthz"]
ENTRYPOINT ["/usr/local/bin/my-ai-gateway"]
