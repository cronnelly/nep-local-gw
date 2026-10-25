# Stage 1: Builder - static musl binary for the build platform (amd64 in CI)
FROM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY . .
RUN cargo build --release --locked --bin nep-gw

# Stage 2: Runtime - Minimal alpine image
FROM alpine:3.24 AS runtime
RUN apk add --no-cache ca-certificates tzdata
COPY --from=builder /app/target/release/nep-gw /usr/local/bin/nep-gw

# Configuration is all environment: PORT (default 80), RUST_LOG, MQTT_HOST,
# MQTT_PORT, MQTT_USERNAME, MQTT_PASSWORD, INVERTER_MODEL, FORWARD_UPSTREAM,
# UPSTREAM_URL. See README.md.
ENV RUST_LOG=info
EXPOSE 80

CMD ["/usr/local/bin/nep-gw"]
