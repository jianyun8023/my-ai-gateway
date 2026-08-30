FROM rust:1.85-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/my-ai-gateway /usr/local/bin/my-ai-gateway
EXPOSE 8787
ENV GATEWAY_LISTEN_ADDR=0.0.0.0:8787
ENTRYPOINT ["/usr/local/bin/my-ai-gateway"]
