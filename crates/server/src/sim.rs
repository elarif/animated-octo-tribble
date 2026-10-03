use crate::routing::{astar, pick_destination, Components, PathCache};
use common::Graph;
use rand::Rng;
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
    SetDensity { set_density: usize },
    Pause { pause: bool },
    GetStats { #[allow(dead_code)] get_stats: serde_json::Value },
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
                let Inner { agents: agent_list, cache, speed, target_agents, next_id, .. } =
                    &mut *inner;
                // Spawn : budget de 100 A*/tick pour éviter un hic au démarrage.
                let mut spawn_budget = 100;
                let target = *target_agents;
                while agent_list.len() < target && spawn_budget > 0 {
                    spawn_budget -= 1;
                    let id = *next_id;
                    *next_id += 1;
                    if let Some(a) = self.spawn_agent(graph, cache, id) {
                        agent_list.push(a);
                    }
                }
                let speed = *speed;
                for agent in agent_list.iter_mut() {
                    self.advance(agent, graph, cache, speed);
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
                inner.target_agents = set_density;
                serde_json::json!({ "target_agents": inner.target_agents })
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
                })
            }
        }
    }

    fn advance(&self, agent: &mut Agent, graph: &Graph, cache: &mut PathCache, speed_mult: f64) {
        if agent.arrived() {
            self.retarget(agent, graph, cache);
            if agent.arrived() {
                return; // retarget impossible, immobile ce tick.
            }
        }
        let mut dist = agent.speed_m_s * speed_mult * 0.1; // tick 100 ms
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

    fn spawn_agent(&self, graph: &Graph, cache: &mut PathCache, id: u64) -> Option<Agent> {
        let mut rng = rand::thread_rng();
        let kind = if rng.gen_bool(0.4) { AgentKind::Pedestrian } else { AgentKind::Car };
        let candidates = match kind {
            AgentKind::Car => &self.car_nodes,
            AgentKind::Pedestrian => &self.ped_nodes,
        };
        if candidates.is_empty() {
            return None;
        }
        let start = pick_destination(candidates, &mut rng);
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

fn agent_in_graph(graph: &Graph, kind: AgentKind, node: u32) -> usize {
    graph.adj[node as usize].iter().filter(|e| kind.filter(e)).count()
}

fn edge_length(graph: &Graph, from: u32, to: u32) -> Option<f64> {
    graph.adj.get(from as usize)?.iter().find(|e| e.to == to).map(|e| e.length_m as f64)
}