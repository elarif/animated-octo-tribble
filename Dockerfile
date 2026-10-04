# syntax=docker/dockerfile:1

# ---- 1. Build du graphe + des binaires ----
FROM rust:1-bookworm AS builder
WORKDIR /app

# Cache des dépendances
COPY Cargo.toml Cargo.lock ./
COPY crates/common/Cargo.toml crates/common/Cargo.toml
COPY crates/osm_to_graph/Cargo.toml crates/osm_to_graph/Cargo.toml
COPY crates/server/Cargo.toml crates/server/Cargo.toml
RUN mkdir -p crates/common/src crates/osm_to_graph/src crates/server/src \
    && echo "fn main() {}" > crates/osm_to_graph/src/main.rs \
    && echo "fn main() {}" > crates/server/src/main.rs \
    && echo "" > crates/common/src/lib.rs \
    && cargo build --release || true

# Vraies sources
COPY crates ./crates
RUN touch crates/server/src/main.rs crates/osm_to_graph/src/main.rs crates/common/src/lib.rs \
    && cargo build --release --workspace

# Données : PBF Geofabrik → graph.bin
RUN curl -sSL -o comoros.osm.pbf https://download.geofabrik.de/africa/comores-latest.osm.pbf \
    && ./target/release/osm_to_graph comoros.osm.pbf graph.bin

# ---- 2. Image finale ----
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/server /app/server
COPY --from=builder /app/graph.bin /app/graph.bin
ENV GRAPH_PATH=/app/graph.bin
ENV PORT=8080
EXPOSE 8080
CMD ["/app/server"]
