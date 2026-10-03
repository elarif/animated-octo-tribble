# Design : Carte Comores + simulation de trafic (milestone 1)

| Champ | Valeur |
|---|---|
| Date | 2026-10-03 |
| Statut | Validé par l'utilisateur en session |
| Périmètre | Milestone 1 uniquement — carte + simulation |
| Hors périmètre | Matching course uber-like, apps mobiles, GPS réel, DB, mode nuit |

## 1. But et contexte

Projet final : une appli uber-like pour mettre en relation voyageurs et taxis
comoriens (trajets A→B). Milestone 1 = socle : la carte des Comores depuis
OpenStreetMap, animée par une circulation simulée de piétons et de voitures.
Ce socle fournit le réseau routier, les trajectoires et l'environnement de test
des milestones suivants (moteur de matching, app mobile).

Décisions validées en session :

- Simulation **semi-réaliste** (B) : densités par zones et classes de routes,
  heures comportementales simples, pas d'analyse de trafic réel.
- Plateforme : **desktop/web Flutter** d'abord, portage mobile au milestone 3.
- Géographie : **toutes les Comores** (Moroni, Mutsamudu…), ~1000 agents cible.
- Rendu carte : **tuiles standard OSM** via `flutter_map` (pas de rendu vectoriel custom).
- Simulation : **backend Rust**, Flutter pur client d'affichage.
- Interaction : **spectateur + contrôles** (pause, vitesse, densité par zone). Le mode
  « acteur » (donner une destination à un agent) arrive au milestone 2.

## 2. Architecture

```
┌─────────────────────┐        WebSocket         ┌──────────────────────┐
│  Flutter desktop    │◄───(ticks 10 Hz)────────►│  Backend Rust        │
│  - flutter_map      │      WS /sim             │  - axum + tokio      │
│  - canvas agents    │                          │  - graphe en mémoire │
│  - panneau ctrl     │      WS /control         │  - moteur sim        │
└─────────────────────┘                          │    (tick 10 Hz)      │
                                                 └─────────┬────────────┘
                                                           │ lit
                                                 ┌─────────▼────────────┐
                                                 │  graph.bin           │
                                                 │  (fichier binaire,   │
                                                 │   généré par parse)  │
                                                 └──────────────────────┘
                                        ▲
                          comoros-latest.osm.pbf (Geofabrik, ~10 Mo)
                          → tool Rust one-shot: parse → graph.bin
```

Deux processus :

1. **`osm_to_graph`** (one-shot) : parse le `.pbf` Geofabrik, produit `graph.bin`.
2. **Serveur** (`axum` + `tokio`) : charge `graph.bin`, exécute le moteur de
   simulation à 10 Hz, expose deux WebSocket.

Flutter est un client passif : dessine ce qu'il reçoit sur `/sim`, envoie des
commandes sur `/control`.

Alternatives envisagées et rejetées :

- Graphe en Postgres/PostGIS : persistance inutile au milestone 1, trois
  composants à opérer. On ajoute Postgres au milestone 2 pour users/courses ;
  le graphe reste en mémoire.
- OSRM/Valhalla pour les déplacements : infra lourde, API non conçue pour
  animer 1000 agents, la simulation resterait à écrire.

## 3. Backend Rust

Dépendances : `axum`, `tokio`, `serde`/`serde_json`, `osmpbf`, `rand`. Rien d'exotique.

### Types du graphe

```rust
struct Node { lat: f64, lon: f64 }
struct Edge { to: u32, length_m: f32, highway_class: HighwayClass, pedestrian: bool, oneway: bool }
// adjacency: Vec<Vec<Edge>> indexée par nœud
```

Vitesse par classe de highway dans une constante partagée (fallback 30 km/h),
`maxspeed` OSM utilisé si présent.

### `osm_to_graph`

1. Lit le `.pbf`, filtre ways avec tag `highway=` (route, path, footway,
   residential, primary…).
2. Nœuds → ids compacts séquentiels ; arêtes dirigées si oneway véhicules ;
   paths/footways réservés piétons.
3. Vitesse par classe, override `maxspeed` OSM.
4. Écrit `graph.bin`.

### Moteur de simulation

- Tick fixe tokio : 100 ms.
- Agent = `{ edge actuelle, position le long de l'edge (0..1), vitesse, destination }`.
- Routage : A* avec heuristique géodésique ; cache LRU des chemins par destination.
- Piétons : ~5 km/h, plus court chemin, uniquement arêtes `pedestrian`.
- Voitures : vitesse de la classe, congestion = ralentissement si plusieurs
  agents sur la même arête.
- Spawn : pondération par classe de highway (residential/primary plus peuplés),
  densité ajustable par bbox.

### WebSocket `/sim` (serveur → client)

Un message JSON par tick :

```json
{"t":1234, "agents":[{"id":1,"k":"car","lat":-11.70,"lon":43.25,"hdg":87.0}]}
```

~1000 agents × ~40 octets ≈ 40 Ko/tick ≈ 400 Ko/s. Passer au binaire seulement
si mesuré insuffisant.

### WebSocket `/control` (client → serveur)

Commandes JSON : `{set_speed:2.0}`, `{set_density:{zone_bbox,count}}`,
`{pause:true}`, `{get_stats}`. Réponses stats sur le même socket. Une commande
invalide renvoie une erreur JSON, non fatale.

## 4. Frontend Flutter

Dépendances : `flutter_map`, `web_socket_channel`, `latlong2`. C'est tout.

Une seule page :

```
┌──────────────────────────────────────────────┐
│              flutter_map (tuiles OSM)        │
│    canvas agents par-dessus                  │
│    points voitures (bleus), piétons (verts)  │
├──────────────────────────────────────────────┤
│ [⏸ Pause] [×2 Vitesse] [Densité: slider]    │
│ Voitures: 412  Piétons: 588   Tick: 8231    │
└──────────────────────────────────────────────┘
```

Composants :

- **`SimMap`** : `FlutterMap` + `CustomPainter` qui dessine les points
  (lat/lon → écran) à chaque frame reçue. Aucun widget par agent (1000 widgets
  = lent, canvas = fluide).
- **`ControlPanel`** : pause, vitesse, slider densité, compteurs live ;
  envoie les commandes JSON sur `/control`.
- **`SimConnection`** : client WS unique ; les ticks alimentent un
  `ChangeNotifier` écouté par le painter ; reconnexion automatique.

Interpolation optionnelle (lerp entre deux ticks pour du 60 fps) : v1 sans,
facile à ajouter.

## 5. Données et flux

- Source : `https://download.geofabrik.de/africa/comoros-latest.osm.pbf`
  (~10 Mo, licence ODbL).
- Format `graph.bin` :

```
[6 octets]  magic "CMGPH1"
[u32]       node_count
[node×16]   lat:f64, lon:f64
[u32]       edge_count (somme des listes d'adjacence)
[edge×12]   to:u32, class:u8, flags:u8, length_m:f32
```

`flags` : bit oneway, bit pedestrian. Le parseur et le serveur partagent un
seul crate de types (`common`), pas de duplication.

Flux :

1. Download Geofabrik (one-shot, curl).
2. `osm_to_graph` → `graph.bin` (~2–5 Mo attendu).
3. Le serveur charge `graph.bin` au boot (~1–2 s), log les stats (nœuds/arêtes).
4. Le client connecte `/sim` + `/control` ; les ticks démarrent dès qu'au
   moins un client `/sim` est connecté.
5. Positions → JSON → WS.

### Gestion d'erreurs

- `graph.bin` absent/invalide : le serveur démarre quand même ; `/sim`
  renvoie `{error:"no_graph"}`.
- PBF corrompu : le parseur avorte avec message clair.
- Déconnexion client : le serveur continue à tick ; reconnexion côté client.
- Densité sur bbox sans routes : erreur renvoyée sur `/control`, non fatale.

## 6. Tests

Minimal, sans framework lourd :

- **Parseur** : petit PBF de test en ressource, assertions sur nœuds/arêtes,
  oneway, pedestrian.
- **Graphe** : round-trip `graph.bin` (octets identiques).
- **A\*** : graphe à la main (10 nœuds), chemin et longueur vs calcul manuel,
  cas « destination inaccessible ».
- **Sim** : 10 agents sur graphe de test — les positions avancent, un piéton
  ne prend jamais une arête non-pedestrian, la congestion ralentit.
- **E2E manuel** : serveur + `wscat` pour vérifier les frames `/sim` et les
  commandes `/control` avant la première connexion Flutter.

## 7. Jalons (ordre de livraison)

1. Parseur PBF → `graph.bin`, log des stats.
2. Serveur : chargement graphe, `/sim` + `/control` fonctionnels (frames vides + stats).
3. Spawn d'agents + déplacement basique (piétons puis voitures, A*).
4. Congestion + densité par zone.
5. Frontend : carte + agents dessinés.
6. Panneau contrôles + polish.

## 8. Révisions

| Date | Description |
|---|---|
| 2026-10-03 | Version initiale, validée en session. |