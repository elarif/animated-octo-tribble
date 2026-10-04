use crate::routing::{astar, pick_destination, Components, PathCache};
use common::Graph;
use rand::Rng;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct SimState {
    graph: Option<Graph>,
    inner: Mutex<Inner>,
    /// Nœuds praticables par type d'agent (précalculés : spawn + destinations).
    car_nodes: Vec<u32>,
    ped_nodes: Vec<u32>,
    /// Composantes connexes : destination même île que le départ.
    car_comp: Option<Components>,
    ped_comp: Option<Components>,
}

struct Inner {
    tick: u64,
    paused: bool,
    speed: f64,
    target_agents: usize,
    agents: Vec<Agent>,
    cache: PathCache,
    next_id: u64,
    /// Densité par zone : bbox (min_lat, min_lon, max_lat, max_lon) → cible.
    /// Zone vide = cible globale pour le reste de la carte.
    zones: Vec<(BBox, usize)>,
}

pub struct Agent {
    pub id: u64,
    pub kind: AgentKind,
    /// Chemin complet start→destination. `path[i]` = nœud courant quand i == idx.
    pub path: Vec<u32>,
    pub idx: usize,
    /// Position le long de l'arête courante (path[idx]→path[idx+1]), 0..1.
    pub t: f64,
    pub speed_m_s: f64,
    /// Zone de densité d'origine (indice dans Inner::zones), None = hors zones.
    pub zone: Option<usize>,
}

impl Agent {
    fn from_node(&self) -> u32 {
        self.path[self.idx]
    }
    fn to_node(&self) -> u32 {
        self.path[self.idx + 1]
    }
    fn arrived(&self) -> bool {
        self.idx + 1 >= self.path.len()
    }
}


#[derive(Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Car,
    Pedestrian,
}

impl AgentKind {
    pub fn filter(self, e: &common::Edge) -> bool {
        match self {
            AgentKind::Car => !e.highway_class.pedestrian_only(),
            AgentKind::Pedestrian => e.pedestrian,
        }
    }
}

#[derive(serde::Serialize, Clone)]
pub struct SimAgent {
    pub id: u64,
    pub k: AgentKind,
    pub lat: f64,
    pub lon: f64,
    pub hdg: f64,
}

#[derive(serde::Serialize, Clone)]
pub struct SimEvent {
    pub t: u64,
    pub agents: Vec<SimAgent>,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
pub enum ControlCommand {
    SetSpeed { set_speed: f64 },
    /// {set_density} = cible globale, ou {set_density:{bbox:[4],count}} = zone.
    SetDensity { set_density: serde_json::Value },
    Pause { pause: bool },
    GetStats { #[allow(dead_code)] get_stats: serde_json::Value },
}

/// Bbox : [min_lat, min_lon, max_lat, max_lon].
type BBox = [f64; 4];

fn in_bbox(node: &common::Node, bbox: &BBox) -> bool {
    node.lat >= bbox[0] && node.lat <= bbox[2] && node.lon >= bbox[1] && node.lon <= bbox[3]
}

impl SimState {
    pub fn load(graph_path: &str) -> Self {
        let graph = match std::fs::read(graph_path) {
            Ok(bytes) => match common::read_graph(&bytes) {
                Ok(g) => {
                    println!("graph chargé : {} nœuds, {} arêtes", g.nodes.len(), g.edge_count());
                    Some(g)
                }
                Err(e) => {
                    eprintln!("graph.bin invalide : {e} — démarrage sans graphe");
                    None
                }
            },
            Err(e) => {
                eprintln!("{graph_path} absent ({e}) — démarrage sans graphe");
                None
            }
        };
        Self::from_graph(graph)
    }

    fn from_graph(graph: Option<Graph>) -> Self {
        // Précalcul composantes (bornage île) + nœuds praticables + destinations pondérées.
        let (car_nodes, ped_nodes, car_comp, ped_comp) = match &graph {
            Some(g) => {
                let mut car = Vec::new();
                let mut ped = Vec::new();
                for (i, edges) in g.adj.iter().enumerate() {
                    if edges.iter().any(|e| !e.highway_class.pedestrian_only()) {
                        car.push(i as u32);
                    }
                    if edges.iter().any(|e| e.pedestrian) {
                        ped.push(i as u32);
                    }
                }
                let cc = Components::build(g, |e| !e.highway_class.pedestrian_only());
                let pc = Components::build(g, |e| e.pedestrian);
                (car, ped, Some(cc), Some(pc))
            }
            None => (Vec::new(), Vec::new(), None, None),
        };
        println!("car_nodes={}, ped_nodes={}", car_nodes.len(), ped_nodes.len());
        let target = if graph.is_some() { 1000 } else { 0 };
        Self {
            graph,
            inner: Mutex::new(Inner {
                tick: 0,
                paused: false,
                speed: 1.0,
                target_agents: target,
                agents: Vec::new(),
                cache: PathCache::new(20_000),
                next_id: 0,
                zones: Vec::new(),
            }),
            car_nodes,
            ped_nodes,
            car_comp,
            ped_comp,
        }
    }

    pub fn has_graph_description(&self) -> &'static str {
        if self.graph.is_some() { "chargé" } else { "ABSENT — /sim renverra no_graph" }
    }

    pub fn tick(&self) -> SimEvent {
        let mut inner = self.inner.lock().unwrap();
        inner.tick += 1;
        let mut agents = Vec::new();
        if let Some(graph) = &self.graph {
            if !inner.paused {
                // Déstructuré pour satisfaire le borrow checker (agents + cache muts séparés).
                let Inner { agents: agent_list, cache, speed, target_agents, next_id, zones, .. } =
                    &mut *inner;
                // Spawn : budget de 100 A*/tick pour éviter un hic au démarrage.
                let mut spawn_budget = 100;
                // Spawn zone d'abord (déficit > 0), puis global jusqu'à target.
                for (i, (_, ztarget)) in zones.iter().enumerate() {
                    let present = agent_list.iter().filter(|a| a.zone == Some(i)).count() as isize;
                    let mut deficit = (*ztarget as isize) - present;
                    while deficit > 0 && spawn_budget > 0 {
                        deficit -= 1;
                        spawn_budget -= 1;
                        let id = *next_id;
                        *next_id += 1;
                        if let Some(a) = self.spawn_agent(graph, cache, id, Some(i), zones) {
                            agent_list.push(a);
                        }
                    }
                }
                while agent_list.iter().filter(|a| a.zone.is_none()).count() < *target_agents
                    && spawn_budget > 0
                {
                    spawn_budget -= 1;
                    let id = *next_id;
                    *next_id += 1;
                    if let Some(a) = self.spawn_agent(graph, cache, id, None, zones) {
                        agent_list.push(a);
                    }
                }
                // Despawn : excès par zone, puis hors zones.
                despawn_excess(agent_list, zones, target_agents);
                let speed = *speed;
                // Congestion : occupation par arête (compte voitures) pour ce tick.
                let mut occupancy: HashMap<(u32, u32), u32> = HashMap::new();
                for a in agent_list.iter() {
                    if a.kind == AgentKind::Car && !a.arrived() {
                        *occupancy.entry((a.from_node(), a.to_node())).or_insert(0) += 1;
                    }
                }
                for agent in agent_list.iter_mut() {
                    self.advance(agent, graph, cache, speed, &occupancy);
                }
            }
            agents = Self::sim_agents(&inner.agents, graph);
        }
        SimEvent { t: inner.tick, agents }
    }

    pub fn control(&self, cmd: ControlCommand) -> serde_json::Value {
        let mut inner = self.inner.lock().unwrap();
        match cmd {
            ControlCommand::SetSpeed { set_speed } => {
                inner.speed = set_speed.max(0.0);
                serde_json::json!({ "speed": inner.speed })
            }
            ControlCommand::SetDensity { set_density } => {
                // Forme 1 : scalaire = cible globale. Forme 2 : zone.
                if let Some(count) = set_density.as_u64() {
                    inner.target_agents = count as usize;
                    serde_json::json!({ "target_agents": inner.target_agents })
                } else if let Ok(z) =
                    serde_json::from_value::<ZoneDensity>(set_density.clone())
                {
                    let bbox = z.bbox;
                    let existing = inner.zones.iter().position(|(b, _)| *b == bbox);
                    if let Some(i) = existing {
                        inner.zones[i].1 = z.count;
                    } else {
                        inner.zones.push((bbox, z.count));
                    }
                    serde_json::json!({ "zones": inner.zones.len() })
                } else {
                    serde_json::json!({ "error": "set_density: attendu nombre ou {bbox:[4],count}" })
                }
            }
            ControlCommand::Pause { pause } => {
                inner.paused = pause;
                serde_json::json!({ "paused": inner.paused })
            }
            ControlCommand::GetStats { .. } => {
                let cars = inner.agents.iter().filter(|a| a.kind == AgentKind::Car).count();
                let peds = inner.agents.len() - cars;
                serde_json::json!({
                    "tick": inner.tick,
                    "cars": cars,
                    "pedestrians": peds,
                    "paused": inner.paused,
                    "speed": inner.speed,
                    "zones": inner.zones.len(),
                })
            }
        }
    }

    fn advance(
        &self,
        agent: &mut Agent,
        graph: &Graph,
        cache: &mut PathCache,
        speed_mult: f64,
        occupancy: &HashMap<(u32, u32), u32>,
    ) {
        if agent.arrived() {
            self.retarget(agent, graph, cache);
            if agent.arrived() {
                return; // retarget impossible, immobile ce tick.
            }
        }
        let mut dist = agent.speed_m_s * speed_mult * 0.1; // tick 100 ms
        // Congestion : seulement les voitures, seulement si elles sont plusieurs
        // sur la même arête dirigée (n voitures → facteur 1/n, ponytail: seuil
        // simple, file d'attente si ce n'est pas assez visuel).
        if agent.kind == AgentKind::Car {
            if let Some(&n) = occupancy.get(&(agent.from_node(), agent.to_node())) {
                if n > 1 {
                    dist /= n as f64;
                }
            }
        }
        loop {
            let (from, to) = (agent.from_node(), agent.to_node());
            let Some(len) = edge_length(graph, from, to) else {
                self.retarget(agent, graph, cache);
                if agent.arrived() || edge_length(graph, agent.from_node(), agent.to_node()).is_none()
                {
                    return;
                }
                continue;
            };
            if len <= 0.0 {
                agent.t = 1.0;
            } else {
                agent.t += dist / len;
            }
            if agent.t < 1.0 {
                return;
            }
            dist -= (1.0 - agent.t.min(1.0).max(0.0)) * len;
            dist = dist.max(0.0);
            dist -= (agent.t - 1.0) * len;
            agent.idx += 1;
            agent.t = 0.0;
            if agent.arrived() {
                self.retarget(agent, graph, cache);
                if agent.arrived() {
                    return;
                }
            }
        }
    }

    /// Nouvelle destination pour l'agent depuis sa position courante.
    fn retarget(&self, agent: &mut Agent, graph: &Graph, cache: &mut PathCache) {
        let mut rng = rand::thread_rng();
        let here = agent.from_node();
        let candidates = self.dest_pool(agent.kind, here);
        for _ in 0..2 {
            let dest = pick_destination(&candidates, &mut rng);
            let path = self.path_to(graph, cache, agent.kind, here, dest);
            if let Some(p) = path {
                agent.path = p;
                agent.idx = 0;
                agent.t = 0.0;
                return;
            }
        }
    }

    /// Nœuds de destination praticables, même composante connexe que `here`
    /// (archipel : pas de trajet inter-îles).
    fn dest_pool(&self, kind: AgentKind, here: u32) -> Vec<u32> {
        let (nodes, comp) = match kind {
            AgentKind::Car => (&self.car_nodes, self.car_comp.as_ref()),
            AgentKind::Pedestrian => (&self.ped_nodes, self.ped_comp.as_ref()),
        };
        match comp {
            Some(c) => {
                let mine = c.component(here);
                nodes.iter().copied().filter(|&n| c.component(n) == mine).collect()
            }
            None => Vec::new(),
        }
    }

    /// A* avec cache. Retourne le chemin complet start→goal ou None.
    fn path_to(
        &self,
        graph: &Graph,
        cache: &mut PathCache,
        kind: AgentKind,
        start: u32,
        goal: u32,
    ) -> Option<Vec<u32>> {
        let key = (start, goal);
        if let Some(hit) = cache.get(key) {
            return hit;
        }
        let path = astar(graph, start, goal, &|e| kind.filter(e));
        cache.put(key, path.clone());
        path
    }

    fn spawn_agent(
        &self,
        graph: &Graph,
        cache: &mut PathCache,
        id: u64,
        zone_idx: Option<usize>,
        zones: &[(BBox, usize)],
    ) -> Option<Agent> {
        let mut rng = rand::thread_rng();
        let kind = if rng.gen_bool(0.4) { AgentKind::Pedestrian } else { AgentKind::Car };
        let candidates_all = match kind {
            AgentKind::Car => &self.car_nodes,
            AgentKind::Pedestrian => &self.ped_nodes,
        };
        if candidates_all.is_empty() {
            return None;
        }
        // Spawn dans la zone de déficit si applicable, sinon n'importe où.
        let candidates: Vec<u32> = match zone_idx.and_then(|i| zones.get(i)) {
            Some((bbox, _)) => candidates_all
                .iter()
                .copied()
                .filter(|&n| in_bbox(&graph.nodes[n as usize], bbox))
                .collect(),
            None => candidates_all.clone(),
        };
        if candidates.is_empty() {
            return None; // zone sans route praticable ; retenté au prochain tick.
        }
        let start = pick_destination(&candidates, &mut rng);
        if agent_in_graph(graph, kind, start) < 2 {
            return None;
        }
        // Destination : même île (composante connexe), pondérée par classe de
        // route (residential/primary plus fréquentés).
        let dest = self.pick_weighted_dest(kind, start, &mut rng)?;
        let path = self.path_to(graph, cache, kind, start, dest)?;
        if path.len() < 2 {
            return None;
        }
        let edge = *graph.adj[start as usize]
            .iter()
            .find(|e| kind.filter(e) && e.to == path[1])?;
        Some(Agent {
            id,
            kind,
            path,
            idx: 0,
            t: rng.gen_range(0.0..1.0),
            speed_m_s: match kind {
                AgentKind::Pedestrian => 5000.0 / 3600.0,
                // ×0.6 : ralentissement moyen urbain (parkings, feux, embouteillages légers).
                AgentKind::Car => edge.highway_class.speed_kmh() as f64 / 3.6 * 0.6,
            },
            zone: zone_idx,
        })
    }

    /// Destination pondérée : weight² pour renforcer la concentration.
    /// ponytail: pondération naïve à chaque appel, si hot-path → précalcul des
    /// tables cumulatives par composante.
    fn pick_weighted_dest(&self, kind: AgentKind, here: u32, rng: &mut impl rand::Rng) -> Option<u32> {
        let (nodes, comp) = match kind {
            AgentKind::Car => (&self.car_nodes, self.car_comp.as_ref()?),
            AgentKind::Pedestrian => (&self.ped_nodes, self.ped_comp.as_ref()?),
        };
        let mine = comp.component(here);
        let pool: Vec<&u32> = nodes.iter().filter(|&&n| comp.component(n) == mine).collect();
        if pool.is_empty() {
            return None;
        }
        let weights: Vec<f64> = pool
            .iter()
            .map(|&&n| {
                let Some(graph) = self.graph.as_ref() else { return 0.0 };
                let w = graph_node_weight(graph, n);
                w * w
            })
            .collect();
        let total: f64 = weights.iter().sum();
        let mut pick = rng.gen_range(0.0..total);
        for (i, &w) in weights.iter().enumerate() {
            pick -= w;
            if pick <= 0.0 {
                return Some(*pool[i]);
            }
        }
        Some(**pool.last().unwrap())
    }

    fn sim_agents(agents: &[Agent], graph: &Graph) -> Vec<SimAgent> {
        agents
            .iter()
            .filter_map(|a| {
                if a.arrived() {
                    return None;
                }
                let na = &graph.nodes[a.from_node() as usize];
                let nb = &graph.nodes[a.to_node() as usize];
                let dlat = nb.lat - na.lat;
                let dlon = nb.lon - na.lon;
                let lat = na.lat + dlat * a.t;
                let lon = na.lon + dlon * a.t;
                let hdg = dlon.atan2(dlat * (na.lat + dlat * 0.5).to_radians().cos()).to_degrees();
                Some(SimAgent { id: a.id, k: a.kind, lat, lon, hdg })
            })
            .collect()
    }
}

/// Poids de population d'un nœud : classe de la meilleure arête adjacente.
/// ponytail: au boot ce serait mieux, appelé dans des boucles de spawn.
fn graph_node_weight(graph: &Graph, node: u32) -> f64 {
    graph.adj[node as usize]
        .iter()
        .map(|e| common::spawn_weight(e.highway_class) as f64)
        .fold(0.5f64, f64::max)
}

#[derive(serde::Deserialize)]
struct ZoneDensity {
    /// [min_lat, min_lon, max_lat, max_lon]
    bbox: BBox,
    count: usize,
}

/// Despawn : excès zone par zone ; hors zones si les zones ont absorbé le
/// budget global (cible globale réduite). Retire le surplus du fond du vec.
fn despawn_excess(agents: &mut Vec<Agent>, zones: &[(BBox, usize)], global_target: &usize) {
    let global_present = agents.iter().filter(|a| a.zone.is_none()).count();
    let mut global_excess = global_present.saturating_sub(*global_target);
    for (i, (_, ztarget)) in zones.iter().enumerate() {
        let mut excess =
            agents.iter().filter(|a| a.zone == Some(i)).count().saturating_sub(*ztarget);
        while excess > 0 {
            let Some(pos) = agents.iter().rposition(|a| a.zone == Some(i)) else { break };
            agents.remove(pos);
            excess -= 1;
        }
    }
    while global_excess > 0 {
        let Some(pos) = agents.iter().rposition(|a| a.zone.is_none()) else { break };
        agents.remove(pos);
        global_excess -= 1;
    }
}

fn agent_in_graph(graph: &Graph, kind: AgentKind, node: u32) -> usize {
    graph.adj[node as usize].iter().filter(|e| kind.filter(e)).count()
}

fn edge_length(graph: &Graph, from: u32, to: u32) -> Option<f64> {
    graph.adj.get(from as usize)?.iter().find(|e| e.to == to).map(|e| e.length_m as f64)
}
#[cfg(test)]
mod tests {
    use super::*;
    use common::{Edge, HighwayClass, Node};

    fn n(lat: f64, lon: f64) -> Node {
        Node { lat, lon }
    }

    fn e(to: u32, len: f32) -> Edge {
        let cls = HighwayClass::Residential;
        Edge { to, length_m: len, highway_class: cls, oneway: false, pedestrian: true }
    }

    /// Chaîne 0-1-2-3 (Residential, praticable voiture+piéton), ~111 m par saut.
    fn test_state() -> SimState {
        let nodes = vec![n(-11.70, 43.24), n(-11.701, 43.24), n(-11.702, 43.24), n(-11.703, 43.24)];
        let mut adj: Vec<Vec<Edge>> = vec![Vec::new(); 4];
        adj[0].push(e(1, 111.0));
        adj[1].push(e(0, 111.0));
        adj[1].push(e(2, 111.0));
        adj[2].push(e(1, 111.0));
        adj[2].push(e(3, 111.0));
        adj[3].push(e(2, 111.0));
        SimState::from_graph(Some(Graph { nodes, adj }))
    }

    #[test]
    fn spawn_cible_globale() {
        let state = test_state();
        state.inner.lock().unwrap().target_agents = 5;
        for _ in 0..10 {
            state.tick();
        }
        let inner = state.inner.lock().unwrap();
        assert_eq!(inner.agents.len(), 5, "spawn jusqu'à la cible");
    }

    #[test]
    fn congestion_raalentit() {
        let state = test_state();
        // Deux voitures sur l'arête 0→1 (facteur 1/2 chacune), une seule sur 2→3.
        {
            let mut inner = state.inner.lock().unwrap();
            inner.target_agents = 3; // pas de spawn : 3 présents.
            inner.agents = vec![
                Agent {
                    id: 1, kind: AgentKind::Car, path: vec![0, 1, 2], idx: 0, t: 0.0,
                    speed_m_s: 8.33, zone: None,
                },
                Agent {
                    id: 2, kind: AgentKind::Car, path: vec![0, 1, 2], idx: 0, t: 0.0,
                    speed_m_s: 8.33, zone: None,
                },
                Agent {
                    id: 3, kind: AgentKind::Car, path: vec![2, 3], idx: 0, t: 0.0,
                    speed_m_s: 8.33, zone: None,
                },
            ];
        }
        state.tick();
        let (t1, t2, t3) = {
            let inner = state.inner.lock().unwrap();
            let t = |id| inner.agents.iter().find(|a| a.id == id).unwrap().t;
            (t(1), t(2), t(3))
        };
        // Congestion : 8.33*0.1/111/2 ≈ 0.00375. Seule : ≈ 0.0075.
        assert!((t1 - t2).abs() < 1e-9, "même arête, même facteur: t1={t1} t2={t2}");
        assert!(t1 < t3, "congestion ralentit: t1={t1} t3={t3}");
        assert!(t1 > 0.0 && t1 < 0.006, "facteur 1/2 appliqué: t1={t1}");
    }

    #[test]
    fn densite_zone_spawn_et_despawn() {
        let state = test_state();
        // Zone bbox couvre nœuds 0,1,2 (pas 3) : 3 agents spawn dans la zone.
        let cmd: ControlCommand = serde_json::from_str(
            r#"{"set_density": {"bbox": [-11.7025, 43.24, -11.70, 43.2405], "count": 3}}"#,
        )
        .unwrap();
        state.control(cmd);
        for _ in 0..10 {
            state.tick();
        }
        let inner = state.inner.lock().unwrap();
        assert_eq!(inner.agents.iter().filter(|a| a.zone == Some(0)).count(), 3);
        drop(inner);
        // Réduction à 0 → despawn complet de la zone.
        let cmd: ControlCommand = serde_json::from_str(
            r#"{"set_density": {"bbox": [-11.7025, 43.24, -11.70, 43.2405], "count": 0}}"#,
        )
        .unwrap();
        state.control(cmd);
        state.inner.lock().unwrap().target_agents = 0; // pas de spawn global.
        state.tick();
        let inner = state.inner.lock().unwrap();
        assert_eq!(inner.agents.iter().filter(|a| a.zone == Some(0)).count(), 0);
    }

    #[test]
    fn set_density_scalaire_compat() {
        let state = test_state();
        let cmd: ControlCommand = serde_json::from_str(r#"{"set_density": 42}"#).unwrap();
        state.control(cmd);
        assert_eq!(state.inner.lock().unwrap().target_agents, 42);
    }
}
