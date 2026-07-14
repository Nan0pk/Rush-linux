FROM rust:latest AS builder

RUN apt-get update && apt-get install -y \
    libdbus-1-dev \
    pkg-config \
    clang \
    cmake \
    libzstd-dev \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/rush-linux
COPY . .

# Build the core binaries that the slim runtime image needs. rushbench and
# the testos-* binaries are excluded because they pull in benchmark-only
# dependencies and are not part of the daemon/CLI runtime surface.
# Note: crates/rush_telemetry is excluded from the workspace in Cargo.toml
# (it does not yet compile cleanly), so it is not built here either.
# See Cargo.toml `exclude` for the rationale.
RUN cargo build --release --bin optid --bin optctl --bin rush-collect

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
