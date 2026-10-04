use common::{haversine, Graph};
use std::collections::{BinaryHeap, HashMap, VecDeque};

/// A* admissible (heuristique = haversine, min 0). Retourne la liste de nœuds
/// `start..=goal`, ou None si inaccessible.
///
/// `filter` : les arêtes praticables par cet agent (piéton vs voiture).
pub fn astar<F>(graph: &Graph, start: u32, goal: u32, filter: &F) -> Option<Vec<u32>>
where
    F: Fn(&common::Edge) -> bool,
{
    if start == goal {
        return Some(vec![start]);
    }
    let goal_node = graph.nodes[goal as usize];

    #[derive(PartialEq)]
    struct Entry {
        f: f64,
        node: u32,
    }
    impl Eq for Entry {}
    impl Ord for Entry {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            other
                .f
                .partial_cmp(&self.f)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
    }
    impl PartialOrd for Entry {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    let mut heap = BinaryHeap::new();
    let mut g_score: HashMap<u32, f64> = HashMap::new();
    let mut came_from: HashMap<u32, u32> = HashMap::new();

    g_score.insert(start, 0.0);
    let h = |n: u32| -> f64 {
        // admissible : arête est au moins aussi longue que la ligne droite.
        haversine(graph.nodes[n as usize], goal_node)
    };
    heap.push(Entry {
        f: h(start),
        node: start,
    });

    while let Some(Entry { f: _, node }) = heap.pop() {
        if node == goal {
            let mut path = vec![goal];
            let mut cur = goal;
            while let Some(&prev) = came_from.get(&cur) {
                path.push(prev);
                cur = prev;
            }
            path.reverse();
            return Some(path);
        }
        let g = g_score.get(&node).copied().unwrap_or(f64::INFINITY);
        for edge in &graph.adj[node as usize] {
            if !filter(edge) {
                continue;
            }
            let tentative = g + edge.length_m as f64;
            if tentative < g_score.get(&edge.to).copied().unwrap_or(f64::INFINITY) {
                g_score.insert(edge.to, tentative);
                came_from.insert(edge.to, node);
                heap.push(Entry {
                    f: tentative + h(edge.to),
                    node: edge.to,
                });
            }
        }
    }
    None
}

/// Cache LRU des chemins, clé `(start, goal)`.
/// ponytail: clé par paire exacte, clé par destination seule si hit rate bas.
pub struct PathCache {
    cap: usize,
    queue: VecDeque<(u32, u32)>,
    map: HashMap<(u32, u32), Option<Vec<u32>>>,
}

impl PathCache {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            queue: VecDeque::new(),
            map: HashMap::new(),
        }
    }

    pub fn get(&mut self, key: (u32, u32)) -> Option<Option<Vec<u32>>> {
        if self.map.contains_key(&key) {
            if let Some(pos) = self.queue.iter().position(|&k| k == key) {
                self.queue.remove(pos);
                self.queue.push_back(key);
            }
            Some(self.map[&key].clone())
        } else {
            None
        }
    }

    pub fn put(&mut self, key: (u32, u32), value: Option<Vec<u32>>) {
        self.map.insert(key, value.clone());
        self.queue.push_back(key);
        if self.queue.len() > self.cap {
            let evicted = self.queue.pop_front();
            if let Some(k) = evicted {
                self.map.remove(&k);
            }
        }
    }
}

/// Choix de la destination : nœud aléatoire parmi les candidats précalculés.
pub fn pick_destination(candidates: &[u32], rng: &mut impl rand::Rng) -> u32 {
    candidates[rng.gen_range(0..candidates.len())]
}

/// Union-find ; composantes connexes précalculées pour borner les destinations
/// à la même île (le pays est un archipel, les composantes = îles).
pub struct Components {
    parent: Vec<u32>,
}

impl Components {
    pub fn build(graph: &Graph, filter: impl Fn(&common::Edge) -> bool + Copy) -> Self {
        let mut parent: Vec<u32> = (0..graph.nodes.len() as u32).collect();
        let find = |p: &[u32], mut x: u32| -> u32 {
            while p[x as usize] != x {
                x = p[x as usize];
            }
            x
        };
        for (i, edges) in graph.adj.iter().enumerate() {
            for e in edges {
                if !filter(e) {
                    continue;
                }
                let a = find(&parent, i as u32);
                let b = find(&parent, e.to);
                if a != b {
                    parent[a as usize] = b;
                }
            }
        }
        // Compressions finales .
        for i in 0..parent.len() {
            let r = find(&parent, i as u32);
            parent[i] = r;
        }
        Self { parent }
    }

    pub fn component(&self, node: u32) -> u32 {
        self.parent[node as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::Node;
    use common::{Edge, HighwayClass};

    fn n(lat: f64, lon: f64) -> Node {
        Node { lat, lon }
    }

    fn e(to: u32, len: f32, class: HighwayClass, oneway: bool) -> Edge {
        let pedestrian = class.pedestrian_ok();
        Edge {
            to,
            length_m: len,
            highway_class: class,
            oneway,
            pedestrian,
        }
    }

    /// Graphe 10 nœuds : chaîne 0-1-2-3-4 + bypass 0-2-4, branche piéton 5-6.
    /// 7 isolé (accessible d'aucun), coordonnées approx Comores pour l'heur.
    fn test_graph() -> Graph {
        let nodes = vec![
            n(-11.7000, 43.2400), // 0
            n(-11.7010, 43.2400), // 1
            n(-11.7020, 43.2400), // 2
            n(-11.7030, 43.2400), // 3
            n(-11.7040, 43.2400), // 4
            n(-11.7000, 43.2410), // 5
            n(-11.7010, 43.2410), // 6
            n(-12.5000, 44.0000), // 7 isolé, loin
        ];
        let mut adj: Vec<Vec<Edge>> = vec![Vec::new(); 8];
        let cls = HighwayClass::Residential;
        adj[0].push(e(1, 111.0, cls, false)); // 0-1
        adj[1].push(e(0, 111.0, cls, false));
        adj[1].push(e(2, 111.0, cls, false));
        adj[2].push(e(1, 111.0, cls, false));
        adj[2].push(e(3, 111.0, cls, false));
        adj[3].push(e(2, 111.0, cls, false));
        adj[3].push(e(4, 111.0, cls, false));
        adj[4].push(e(3, 111.0, cls, false));
        // bypass 0-2 direct (222 m = 2×111) : chemin 0-2 = 0-1-2 aussi 222. tie.
        adj[0].push(e(2, 222.0, cls, false));
        adj[2].push(e(0, 222.0, cls, false));
        // piéton only 5-6, 5-0
        adj[5].push(e(6, 74.0, HighwayClass::Footway, false));
        adj[6].push(e(5, 74.0, HighwayClass::Footway, false));
        adj[5].push(e(0, 74.0, HighwayClass::Footway, false)); // hmm, footway est pedestrian_only. En vrai c'est OK: piéton peut marcher dessus. Mais une "Edge" footway a pedestrian=true (via pedestrian_ok, Footway => true ici car Footway n'est pas Motorway/Trunk).
        adj[0].push(e(5, 74.0, HighwayClass::Footway, false));
        Graph { nodes, adj }
    }

    const CAR: fn(&Edge) -> bool = |e: &Edge| !e.highway_class.pedestrian_only();
    const PED: fn(&Edge) -> bool = |e: &Edge| e.pedestrian;

    #[test]
    fn chemin_simple() {
        let g = test_graph();
        let path = astar(&g, 0, 4, &CAR).unwrap();
        assert_eq!(path[0], 0);
        assert_eq!(path[path.len() - 1], 4);
        // longueur : soit 444 (0-1-2-3-4), soit 222+222+... vérifions que <= 444
        let len: f32 = path
            .windows(2)
            .map(|w| {
                g.adj[w[0] as usize]
                    .iter()
                    .find(|e| e.to as usize == w[1] as usize)
                    .unwrap()
                    .length_m
            })
            .sum();
        assert!(len <= 444.5, "len={len}");
        assert!(len >= 333.0, "len={len}");
    }

    #[test]
    fn inaccessible() {
        let g = test_graph();
        assert!(astar(&g, 0, 7, &CAR).is_none(), "7 isolé");
    }

    #[test]
    fn pieton_ne_prend_pas_footway_inexistant() {
        let g = test_graph();
        // 7 est accessible d'aucune façon.
        assert!(astar(&g, 0, 7, &PED).is_none());
        // mais 5-6 est praticable à pied.
        assert!(astar(&g, 5, 6, &PED).is_some());
        // et les voitures ne peuvent pas aller sur les footways : 0→5 en voiture impossible si
        // l'arête 0-5 est un Footway (pedestrian_only).
        assert!(
            astar(&g, 0, 5, &CAR).is_none() || g.adj[0].iter().any(|e| e.to == 5_u32 && CAR(e))
        );
    }

    #[test]
    fn meme_noeud() {
        let g = test_graph();
        assert_eq!(astar(&g, 3, 3, &CAR), Some(vec![3]));
    }

    #[test]
    fn cache_roundtrip() {
        let mut c = PathCache::new(2);
        c.put((1, 2), Some(vec![1, 2]));
        c.put((2, 3), None);
        assert_eq!(c.get((1, 2)), Some(Some(vec![1, 2])));
        assert_eq!(c.get((2, 3)), Some(None));
        assert_eq!(c.get((3, 4)), None);
        // éviction : (1,2) doît sortir après insertion de 2 nouvelles clés.
        c.put((3, 4), Some(vec![3, 4]));
        c.put((4, 5), Some(vec![4, 5]));
        assert!(c.get((1, 2)).is_none(), "éviction LRU");
        assert_eq!(c.get((4, 5)), Some(Some(vec![4, 5])));
    }
}
