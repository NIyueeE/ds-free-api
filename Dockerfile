FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app

COPY target/x86_64-unknown-linux-gnu/release/ds-free-api /app/ds-free-api
COPY web/dist /app/web/dist
COPY config.example.toml /app/config.example.toml

VOLUME /app/data

ENV RUST_LOG=info
ENV DS_DATA_DIR=/app/data

EXPOSE 5317

ENTRYPOINT ["/app/ds-free-api"]
