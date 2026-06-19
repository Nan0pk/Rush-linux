FROM rust:1.78-slim-bookworm AS builder

RUN apt-get update && apt-get install -y \
    libdbus-1-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/rush-linux
COPY . .

RUN cargo build --release --workspace

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    dbus \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/rush-linux/target/release/optid /usr/local/bin/
COPY --from=builder /usr/src/rush-linux/target/release/optctl /usr/local/bin/
COPY --from=builder /usr/src/rush-linux/target/release/rush-collect /usr/local/bin/

# Default policy
RUN mkdir -p /usr/lib/optid
COPY --from=builder /usr/src/rush-linux/config/optid/policy.toml /usr/lib/optid/

ENTRYPOINT ["optid"]
