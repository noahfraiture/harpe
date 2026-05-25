FROM rust:1-bookworm AS builder

WORKDIR /app
COPY . .
RUN cargo build --release -p harpe-server

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/harpe-server /usr/local/bin/harpe-server

ENV HARPE_GRPC_ADDR=0.0.0.0:50051
EXPOSE 50051

CMD ["harpe-server"]
