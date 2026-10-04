# Simulation de trafic Comores

Carte des Comores (OSM) animée par une circulation simulée de piétons et de
voitures. Voir `docs/superpowers/specs/2026-10-03-comoros-traffic-sim-design.md`.

## Composants

- `crates/osm_to_graph` — one-shot : PBF Geofabrik → `data/graph.bin`
- `crates/server` — axum : WS `/sim` (ticks 10 Hz) + WS `/control` (commandes)
- `app/` — client Flutter (web / Linux desktop), flutter_map + canvas agents

## Mise en route

```sh
# 1. Extraire le graphe (une fois)
curl -O https://download.geofabrik.de/africa/comores-latest.osm.pbf
cargo run --release -p osm_to_graph -- comoros-latest.osm.pbf data/graph.bin

# 2. Serveur de simulation
GRAPH_PATH=data/graph.bin ADDR=127.0.0.1:9000 cargo run --release -p server

# 3. Client Flutter (web)
cd app && flutter run -d web-server --web-port 8080
# ou build statique : flutter build web && python3 -m http.server -d build/web
```

`/sim` émet `{"t":..., "agents":[{"id","k","lat","lon","hdg"}]}` ;
`/control` accepte `{set_speed}`, `{set_density}`, `{pause}`, `{get_stats}`.

## Tests

```sh
cargo test --workspace
cd app && flutter analyze
```
