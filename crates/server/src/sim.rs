use common::Graph;
use rand::Rng;
use std::sync::Mutex;

pub struct SimState {
    graph: Option<Graph>,
    inner: Mutex<Inner>,
}

struct Inner {
    tick: u64,
    paused: bool,
    speed: f64,
    target_agents: usize,
    agents: Vec<Agent>,
}

pub struct Agent {
    pub id: u64,
    pub kind: AgentKind,
    pub from: u32,
    pub to: u32,
    pub t: f64, // position le long de l'arête, 0..1
    pub speed_m_s: f64,
}

#[derive(Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Car,
    Pedestrian,
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
    GetStats { get_stats: serde_json::Value },
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
        let target = if graph.is_some() { 1000 } else { 0 };
        Self {
            graph,
            inner: Mutex::new(Inner {
                tick: 0,
                paused: false,
                speed: 1.0,
                target_agents: target,
                agents: Vec::new(),
            }),
        }
    }

    pub fn has_graph_description(&self) -> &'static str {
        if self.graph.is_some() { "chargé" } else { "ABSENT — /sim renverra no_graph" }
    }

    pub fn tick(&self) -> SimEvent {
        let mut inner = self.inner.lock().unwrap();
        inner.tick += 1;
        let mut agents = Vec::new();
        if !inner.paused {
            if let Some(graph) = &self.graph {
                let speed = inner.speed;
                // spawn jusqu'à la cible.
                while inner.agents.len() < inner.target_agents {
                    if let Some(a) = spawn_agent(graph, inner.agents.len() as u64) {
                        inner.agents.push(a);
                    } else {
                        break;
                    }
                }
                // déplacement.
                for agent in &mut inner.agents {
                    advance(agent, graph, speed);
                }
            }
            agents = sim_agents(&inner.agents, &self.graph);
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
}

fn sim_agents(agents: &[Agent], graph: &Option<Graph>) -> Vec<SimAgent> {
    let Some(graph) = graph else { return Vec::new() };
    agents
        .iter()
        .filter_map(|a| {
            let na = graph.nodes.get(a.from as usize)?;
            let nb = graph.nodes.get(a.to as usize)?;
            let dlat = nb.lat - na.lat;
            let dlon = nb.lon - na.lon;
            let lat = na.lat + dlat * a.t;
            let lon = na.lon + dlon * a.t;
            let hdg = dlon.atan2(dlat * (na.lat + dlat * 0.5).to_radians().cos()).to_degrees();
            Some(SimAgent { id: a.id, k: a.kind, lat, lon, hdg })
        })
        .collect()
}

fn edge_length(graph: &Graph, from: u32, to: u32) -> Option<f64> {
    graph.adj.get(from as usize)?.iter().find(|e| e.to == to).map(|e| e.length_m as f64)
}

fn advance(agent: &mut Agent, graph: &Graph, speed_mult: f64) {
    let Some(len) = edge_length(graph, agent.from, agent.to) else {
        // arête disparue : reset.
        agent.t = 1.0;
        return;
    };
    let target = if agent.t == 1.0 { 0.0 } else { 1.0 };
    let _ = target;
    let dist = agent.speed_m_s * speed_mult * 0.1; // tick 100 ms
    let step = if len > 0.0 { dist / len } else { 1.0 };
    agent.t += step;
    while agent.t >= 1.0 {
        // arriver au bout : prendre le prochain nœud du chemin.
        agent.t -= 1.0;
        agent.from = agent.to;
        // TODO jalon 3 : A* pour choisir le prochain segment via path.
        if let Some(next) = graph.adj[agent.from as usize]
            .iter()
            .filter(|e| match agent.kind {
                AgentKind::Car => true,
                AgentKind::Pedestrian => e.pedestrian,
            })
            .map(|e| e.to)
            .next()
        {
            agent.to = next;
        } else {
            // cul-de-sac : demi-tour.
            agent.to = agent.from;
            break;
        }
    }
}

fn spawn_agent(graph: &Graph, id: u64) -> Option<Agent> {
    let mut rng = rand::thread_rng();
    let kind = if rng.gen_bool(0.4) { AgentKind::Pedestrian } else { AgentKind::Car };
    // Piétons : arêtes pedestrian ; voitures : toutes.
    let candidates: Vec<u32> = graph
        .adj
        .iter()
        .enumerate()
        .filter(|(_, edges)| {
            !edges.is_empty()
                && edges.iter().any(|e| match kind {
                    AgentKind::Pedestrian => e.pedestrian,
                    AgentKind::Car => true,
                })
        })
        .map(|(i, _)| i as u32)
        .collect();
    let from = *candidates.get(rng.gen_range(0..candidates.len()))?;
    let edges = graph.adj.get(from as usize)?;
    let pool: Vec<&common::Edge> = edges
        .iter()
        .filter(|e| match kind {
            AgentKind::Pedestrian => e.pedestrian,
            AgentKind::Car => true,
        })
        .collect();
    let e = pool.get(rng.gen_range(0..pool.len()))?;
    Some(Agent {
        id,
        kind,
        from,
        to: e.to,
        t: rng.gen_range(0.0..1.0),
        speed_m_s: match kind {
            AgentKind::Pedestrian => 5000.0 / 3600.0,
            AgentKind::Car => e.highway_class.speed_kmh() as f64 / 3.6,
        },
    })
}