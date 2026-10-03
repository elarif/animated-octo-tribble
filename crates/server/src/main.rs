mod sim;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use sim::{ControlCommand, SimEvent, SimState};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;

#[tokio::main]
async fn main() {
    let graph_path = std::env::var("GRAPH_PATH").unwrap_or_else(|_| "data/graph.bin".into());
    let addr: SocketAddr = std::env::var("ADDR")
        .unwrap_or_else(|_| "127.0.0.1:9000".into())
        .parse()
        .expect("ADDR invalide");

    let state = Arc::new(SimState::load(&graph_path));
    println!(
        "serveur {addr} — graph: {}",
        state.has_graph_description()
    );

    let (sim_tx, _) = broadcast::channel::<Arc<SimEvent>>(64);

    let app = Router::new()
        .route("/sim", get(sim_ws))
        .route("/control", get(control_ws))
        .with_state((state.clone(), sim_tx.clone()));

    // Boucle de tick 10 Hz.
    tokio::spawn({
        let state = state.clone();
        let sim_tx = sim_tx.clone();
        async move { tick_loop(state, sim_tx).await }
    });

    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    axum::serve(listener, app).await.expect("serve");
}

async fn tick_loop(state: Arc<SimState>, sim_tx: broadcast::Sender<Arc<SimEvent>>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    loop {
        interval.tick().await;
        let event = state.tick();
        let _ = sim_tx.send(Arc::new(event));
    }
}

async fn sim_ws(
    axum::extract::State((_, sim_tx)): axum::extract::State<(Arc<SimState>, broadcast::Sender<Arc<SimEvent>>)>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_sim(socket, sim_tx))
}

async fn handle_sim(mut socket: WebSocket, sim_tx: broadcast::Sender<Arc<SimEvent>>) {
    let mut rx = sim_tx.subscribe();
    loop {
        tokio::select! {
            res = rx.recv() => {
                match res {
                    Ok(event) => {
                        if socket.send(Message::Text(serde_json::to_string(&*event).unwrap().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("/sim client en retard, {n} ticks perdus");
                    }
                    Err(_) => break,
                }
            }
            res = socket.recv() => {
                if res.is_none() { break; } // client parti
            }
        }
    }
}

async fn control_ws(
    axum::extract::State((state, _)): axum::extract::State<(Arc<SimState>, broadcast::Sender<Arc<SimEvent>>)>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_control(socket, state))
}

async fn handle_control(mut socket: WebSocket, state: Arc<SimState>) {
    while let Some(Ok(Message::Text(txt))) = socket.recv().await {
        let reply: Result<serde_json::Value, String> = match serde_json::from_str::<ControlCommand>(&txt) {
            Ok(cmd) => Ok(state.control(cmd)),
            Err(e) => Err(format!("commande invalide: {e}")),
        };
        let payload = match reply {
            Ok(v) => v,
            Err(e) => serde_json::json!({ "error": e }),
        };
        if socket.send(Message::Text(payload.to_string().into())).await.is_err() {
            break;
        }
    }
}