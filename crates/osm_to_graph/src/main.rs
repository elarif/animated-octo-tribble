use anyhow::{bail, Context, Result};
use common::{write_graph, Edge, Graph, HighwayClass, Node};
use osmpbf::{Element, ElementReader};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::time::Instant;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        bail!("usage: osm_to_graph <input.pbf> <output.graph.bin>");
    }
    let start = Instant::now();
    let graph = parse(&args[1])?;
    fs::write(&args[2], write_graph(&graph)).context("écriture graph.bin")?;
    println!(
        "{} nœuds, {} arêtes en {:.1?} → {}",
        graph.nodes.len(),
        graph.edge_count(),
        start.elapsed(),
        args[2]
    );
    Ok(())
}

type Way = (Vec<i64>, HighwayClass, bool);

fn parse(path: &str) -> Result<Graph> {
    let mut ways: Vec<Way> = Vec::new();

    let tag = |w: &osmpbf::Way, key: &str| -> Option<String> {
        w.tags()
            .filter(|(k, _)| *k == key)
            .map(|(_, v)| v.to_string())
            .next()
    };

    ElementReader::from_path(path)
        .with_context(|| format!("ouverture {path}"))?
        .for_each(|element| {
            if let Element::Way(w) = element {
                let class = tag(&w, "highway").and_then(|t| HighwayClass::from_tag(&t));
                let Some(class) = class else { return };
                if tag(&w, "access").as_deref() == Some("no") && !class.pedestrian_only() {
                    return;
                }
                let oneway = tag(&w, "oneway")
                    .map(|t| t == "yes" || t == "1" || t == "true")
                    .unwrap_or(false);
                ways.push((w.refs().collect(), class, oneway));
            }
        })?;

    // Ids de nœuds utilisés → indices compacts.
    let mut used: Vec<i64> = ways.iter().flat_map(|(n, _, _)| n.iter().copied()).collect();
    used.sort_unstable();
    used.dedup();
    let mut osm_id_to_idx: HashMap<i64, u32> = HashMap::with_capacity(used.len());
    for (i, &id) in used.iter().enumerate() {
        osm_id_to_idx.insert(id, i as u32);
    }
    let mut nodes = vec![Node { lat: 0.0, lon: 0.0 }; used.len()];

    // Coordonnées : deuxième lecteur sur le même fichier.
    let mut count = 0usize;
    ElementReader::from_path(path)
        .with_context(|| format!("ouverture {path}"))?
        .for_each(|element| {
            let (id, lat, lon) = match element {
                Element::DenseNode(n) => (n.id(), n.lat() as f64, n.lon() as f64),
                Element::Node(n) => (n.id(), n.lat() as f64, n.lon() as f64),
                _ => return,
            };
            if let Some(&idx) = osm_id_to_idx.get(&id) {
                nodes[idx as usize] = Node { lat, lon };
                count += 1;
            }
        })?;
    if count < used.len() {
        bail!(
            "nœuds manquants dans le pbf : {}/{}, fichier probablement corrompu",
            count,
            used.len()
        );
    }

    // Adjacence.
    let mut adj: Vec<Vec<Edge>> = vec![Vec::new(); used.len()];
    for (nids, class, oneway) in &ways {
        for win in nids.windows(2) {
            let (ai, bi) = match (
                osm_id_to_idx.get(&win[0]),
                osm_id_to_idx.get(&win[1]),
            ) {
                (Some(&a), Some(&b)) => (a as usize, b as usize),
                _ => continue,
            };
            let length_m = haversine(nodes[ai], nodes[bi]);
            let pedestrian = class.pedestrian_ok();
            adj[ai].push(Edge {
                to: bi as u32,
                length_m,
                highway_class: *class,
                oneway: *oneway,
                pedestrian,
            });
            if !*oneway {
                adj[bi].push(Edge {
                    to: ai as u32,
                    length_m,
                    highway_class: *class,
                    oneway: false,
                    pedestrian,
                });
            }
        }
    }
    Ok(Graph { nodes, adj })
}

fn haversine(a: Node, b: Node) -> f32 {
    let r = 6_371_000.0;
    let dlat = (b.lat - a.lat).to_radians();
    let dlon = (b.lon - a.lon).to_radians();
    let h = (dlat / 2.0).sin().powi(2)
        + a.lat.to_radians().cos() * b.lat.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    (2.0 * r * h.sqrt()) as f32
}