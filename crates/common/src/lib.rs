use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum HighwayClass {
    Motorway = 0,
    Trunk = 1,
    Primary = 2,
    Secondary = 3,
    Tertiary = 4,
    Residential = 5,
    Unclassified = 6,
    Service = 7,
    Track = 8,
    Path = 9,
    Footway = 10,
    Steps = 11,
}

impl HighwayClass {
    /// Vitesse km/h par classe, fallback 30 pour les inconnues.
    pub fn speed_kmh(self) -> u32 {
        match self {
            HighwayClass::Motorway => 90,
            HighwayClass::Trunk => 70,
            HighwayClass::Primary => 60,
            HighwayClass::Secondary => 50,
            HighwayClass::Tertiary => 40,
            HighwayClass::Residential => 30,
            HighwayClass::Unclassified => 30,
            HighwayClass::Service => 20,
            HighwayClass::Track => 15,
            HighwayClass::Path | HighwayClass::Footway | HighwayClass::Steps => 0,
        }
    }

    /// Les classes réservées aux piétons (pas de véhicules motorisés).
    pub fn pedestrian_only(self) -> bool {
        matches!(
            self,
            HighwayClass::Path | HighwayClass::Footway | HighwayClass::Steps
        )
    }

    /// Piéton autorisé : classe piéton-pure ou route mixte.
    pub fn pedestrian_ok(self) -> bool {
        !matches!(self, HighwayClass::Motorway | HighwayClass::Trunk)
    }

    pub fn from_tag(tag: &str) -> Option<HighwayClass> {
        Some(match tag {
            "motorway" | "motorway_link" => HighwayClass::Motorway,
            "trunk" | "trunk_link" => HighwayClass::Trunk,
            "primary" | "primary_link" => HighwayClass::Primary,
            "secondary" | "secondary_link" => HighwayClass::Secondary,
            "tertiary" | "tertiary_link" => HighwayClass::Tertiary,
            "residential" | "living_street" | "unclassified" => HighwayClass::Residential,
            "service" => HighwayClass::Service,
            "track" => HighwayClass::Track,
            "path" | "bridleway" | "cycleway" => HighwayClass::Path,
            "footway" | "pedestrian" | "corridor" => HighwayClass::Footway,
            "steps" => HighwayClass::Steps,
            _ => return None,
        })
    }
}

/// Poids des classes pour la densité de population, utilisé pour le spawn.
pub fn spawn_weight(class: HighwayClass) -> f32 {
    match class {
        HighwayClass::Residential => 5.0,
        HighwayClass::Primary => 4.0,
        HighwayClass::Secondary => 3.0,
        HighwayClass::Tertiary => 3.0,
        HighwayClass::Unclassified => 2.0,
        HighwayClass::Service | HighwayClass::Track => 1.0,
        _ => 0.5,
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Node {
    pub lat: f64,
    pub lon: f64,
}

pub const FLAG_ONEWAY: u8 = 1;
pub const FLAG_PEDESTRIAN: u8 = 2;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Edge {
    pub to: u32,
    pub length_m: f32,
    pub highway_class: HighwayClass,
    pub oneway: bool,
    pub pedestrian: bool,
}

impl Edge {
    /// Vitesse effective km/h (maxspeed non géré au parse : fallback classe).
    pub fn speed_kmh(&self) -> u32 {
        self.highway_class.speed_kmh()
    }

    pub fn is_walkable(&self) -> bool {
        self.pedestrian
    }
}

pub const MAGIC: &[u8; 6] = b"CMGPH1";

#[derive(Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    /// Liste d'adjacence indexée par nœud.
    pub adj: Vec<Vec<Edge>>,
}

impl Graph {
    pub fn edge_count(&self) -> usize {
        self.adj.iter().map(|a| a.len()).sum()
    }
}

/// Distance géodésique en mètres.
pub fn haversine(a: Node, b: Node) -> f64 {
    let r = 6_371_000.0;
    let dlat = (b.lat - a.lat).to_radians();
    let dlon = (b.lon - a.lon).to_radians();
    let h = (dlat / 2.0).sin().powi(2)
        + a.lat.to_radians().cos() * b.lat.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    2.0 * r * h.sqrt()
}

/// Sérialisation bincode-like simple : JSON dans un Vec, avec magic + comptes.
/// ponytail: bincode ajouté si la taille du fichier pose un problème.
pub fn write_graph(graph: &Graph) -> Vec<u8> {
    let payload = bincode::serialize(graph).expect("serialize graph");
    let mut out = Vec::with_capacity(payload.len() + 12);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

pub fn read_graph(bytes: &[u8]) -> Result<Graph, String> {
    if bytes.len() < 14 || &bytes[..6] != MAGIC {
        return Err("magic invalide".to_string());
    }
    let payload_len = u64::from_le_bytes(bytes[6..14].try_into().unwrap()) as usize;
    if bytes.len() < 14 + payload_len {
        return Err("fichier tronqué".to_string());
    }
    bincode::deserialize(&bytes[14..14 + payload_len]).map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let g = Graph {
            nodes: vec![Node { lat: -11.7, lon: 43.25 }, Node { lat: -11.71, lon: 43.26 }],
            adj: vec![
                vec![Edge { to: 1, length_m: 1234.5, highway_class: HighwayClass::Residential, oneway: true, pedestrian: true }],
                vec![Edge { to: 0, length_m: 1234.5, highway_class: HighwayClass::Residential, oneway: false, pedestrian: true }],
            ],
        };
        let bytes = write_graph(&g);
        let g2 = read_graph(&bytes).unwrap();
        assert_eq!(g.nodes.len(), g2.nodes.len());
        assert_eq!(g.adj.len(), g2.adj.len());
        assert!((g2.adj[0][0].length_m - 1234.5).abs() < 0.01);
        assert_eq!(g2.adj[0][0].highway_class, HighwayClass::Residential);
        assert!(g2.adj[0][0].oneway);
    }
}
