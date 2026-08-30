FROM node:24-bookworm AS web-builder
WORKDIR /app/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web ./
RUN npm run build

FROM rust:1.85-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --locked
COPY --from=web-builder /app/web/dist /app/web/dist

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /opt/my-ai-gateway
COPY --from=builder /app/target/release/my-ai-gateway /usr/local/bin/my-ai-gateway
COPY --from=builder /app/web/dist ./web/dist
EXPOSE 8787
ENV GATEWAY_LISTEN_ADDR=0.0.0.0:8787
ENTRYPOINT ["/usr/local/bin/my-ai-gateway"]
