use crate::server::metadata::MetadataStore;
use crate::server::storage::{ArtifactStorage, LocalStorage};
use crate::storage::storage_from_env;
use anyhow::Result;
use axum::{
    body::Bytes,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, head, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde::Serialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
// use tower_governor::GovernorLayer;

pub mod metadata;
pub mod storage;

pub struct AppState {
    pub metadata: MetadataStore,
    pub storage: Arc<dyn ArtifactStorage>,
    pub webhook_url: Option<String>,
    pub tx_events: broadcast::Sender<crate::dashboard::BuildEvent>,
    pub current_dag: Arc<std::sync::Mutex<Option<crate::graph::BuildGraph>>>,
    pub auth_state: Arc<crate::auth::AuthState>,
    pub metrics: crate::metrics::SharedMetrics,
}

#[derive(Deserialize)]
pub struct GcQuery {
    pub days: u32,
}

#[derive(Deserialize, Clone)]
pub struct AnalyticsData {
    pub dirty: u32,
    pub cached: u32,
    pub duration_ms: u64,
}

async fn add_api_version_header<B>(req: Request<B>, next: Next<B>) -> Response {
    let mut response = next.run(req).await;
    response.headers_mut().insert(
        "X-MemoBuild-API-Version",
        axum::http::HeaderValue::from_static("1.0"),
    );
    response
}

pub async fn start_server(
    port: u16,
    data_dir: PathBuf,
    webhook_url: Option<String>,
    tls_config: Option<crate::tls::TlsConfig>,
    admin_token: Option<String>,
    auth_db_client: Option<tokio_postgres::Client>,
) -> Result<()> {
    let db_path = data_dir.join("metadata.db");
    let metadata = MetadataStore::new(&db_path)?;
    let storage: Arc<dyn ArtifactStorage> = match storage_from_env(&data_dir) {
        Ok(s) => Arc::from(s),
        Err(_) => Arc::new(LocalStorage::new(&data_dir)?),
    };

    let (tx_events, _) = broadcast::channel(crate::constants::MAX_WS_BROADCAST_CAPACITY);
    let current_dag = Arc::new(std::sync::Mutex::new(None));

    let auth_state = Arc::new(crate::auth::AuthState::new(admin_token, auth_db_client));
    let metrics = crate::metrics::metrics_registry();

    let state = Arc::new(AppState {
        metadata,
        storage,
        webhook_url,
        tx_events,
        current_dag,
        auth_state,
        metrics,
    });

    let app = Router::new()
        .route("/", get(dashboard))
        .route("/cache/:hash", head(check_cache))
        .route("/cache/:hash", get(get_artifact))
        .route("/cache/:hash", put(put_artifact))
        // Layered cache routes
        .route("/cache/layer/:hash", head(check_layer))
        .route("/cache/layer/:hash", get(get_layer))
        .route("/cache/layer/:hash", put(put_layer))
        .route("/cache/node/:hash/layers", get(get_node_layers))
        .route("/cache/node/:hash/layers", post(register_node_layers))
        .route("/gc", post(gc_cache))
        .route("/gc/status", get(gc_status))
        .route("/metrics", get(metrics_handler))
        .route("/analytics", post(report_analytics))
        .route("/build-event", post(receive_build_event))
        .route("/dag", post(register_dag))
        .route("/dag", get(get_dag))
        .route("/api/analytics", get(get_analytics_handler))
        .route("/api/layers", get(get_layer_stats_handler))
        .route("/ws", get(ws_handler))
        .layer(middleware::from_fn(add_api_version_header))
        // Add auth routes
        
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("🌐 MemoBuild Remote Cache Server running on {}", addr);

    if let Some(tls) = tls_config {
        let rustls_config = tls.axum_rustls_config()?;
        axum_server::bind_rustls(addr, rustls_config)
            .serve(app.into_make_service())
            .await?;
    } else {
        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await?;
    }

    Ok(())
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.tx_events.subscribe();

    while let Ok(event) = rx.recv().await {
        let msg = serde_json::to_string(&event).unwrap_or_default();
        if socket.send(Message::Text(msg)).await.is_err() {
            break;
        }
    }
}

async fn receive_build_event(
    State(state): State<Arc<AppState>>,
    Json(event): Json<crate::dashboard::BuildEvent>,
) -> impl IntoResponse {
    let _ = state.tx_events.send(event);
    StatusCode::OK
}

async fn register_dag(
    State(state): State<Arc<AppState>>,
    Json(dag): Json<crate::graph::BuildGraph>,
) -> impl IntoResponse {
    let mut current_dag = state.current_dag.lock().unwrap();
    *current_dag = Some(dag);
    StatusCode::OK
}

#[derive(Serialize)]
struct DagResponse {
    nodes: Vec<crate::graph::Node>,
    edges: Vec<(usize, usize)>,
}

async fn get_dag(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let dag_lock = state.current_dag.lock().unwrap();
    match &*dag_lock {
        Some(dag) => {
            let mut edges = Vec::new();
            for (i, node) in dag.nodes.iter().enumerate() {
                for &dep in &node.deps {
                    edges.push((dep, i));
                }
            }
            let resp = DagResponse {
                nodes: dag.nodes.clone(),
                edges,
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn get_analytics_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state
        .metadata
        .get_analytics(crate::constants::ANALYTICS_DB_LIMIT as u32)
    {
        Ok(data) => (StatusCode::OK, Json(data)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_layer_stats_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.metadata.get_layer_stats() {
        Ok(data) => (StatusCode::OK, Json(data)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn dashboard() -> Html<String> {
    let html = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>MemoBuild Dashboard</title>
    <link href="https://fonts.googleapis.com/css2?family=Outfit:wght@300;400;600&display=swap" rel="stylesheet">
    <script src="https://unpkg.com/vis-network/standalone/umd/vis-network.min.js"></script>
    <style>
        :root {
            --bg: #0f172a;
            --card: #1e293b;
            --accent: #38bdf8;
            --text: #f1f5f9;
            --text-dim: #94a3b8;
            --success: #22c55e;
            --warning: #f59e0b;
            --error: #ef4444;
            --building: #818cf8;
        }
        body {
            font-family: 'Outfit', sans-serif;
            background-color: var(--bg);
            color: var(--text);
            margin: 0;
            padding: 2rem;
            display: flex;
            flex-direction: column;
            align-items: center;
        }
        .container {
            max-width: 1200px;
            width: 100%;
        }
        h1 {
            font-size: 2.5rem;
            margin: 0;
            background: linear-gradient(to right, #38bdf8, #818cf8);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }
        .subtitle {
            color: var(--text-dim);
            margin-bottom: 2rem;
        }
        .grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 1.5rem;
            margin-bottom: 2rem;
        }
        .card {
            background: var(--card);
            padding: 1.5rem;
            border-radius: 1rem;
            border: 1px solid rgba(255, 255, 255, 0.1);
            transition: transform 0.2s;
        }
        .stat-value {
            font-size: 2rem;
            font-weight: 600;
            color: var(--accent);
        }
        .stat-label {
            color: var(--text-dim);
            font-size: 0.9rem;
            text-transform: uppercase;
            letter-spacing: 0.05em;
        }
        #dag-container {
            width: 100%;
            height: 500px;
            background: var(--card);
            border-radius: 1rem;
            border: 1px solid rgba(255, 255, 255, 0.1);
            margin-bottom: 2rem;
        }
        table {
            width: 100%;
            border-collapse: collapse;
            background: var(--card);
            border-radius: 1rem;
            overflow: hidden;
        }
        th {
            text-align: left;
            padding: 1rem;
            background: rgba(255, 255, 255, 0.05);
            color: var(--text-dim);
            font-weight: 600;
        }
        td {
            padding: 1rem;
            border-top: 1px solid rgba(255, 255, 255, 0.05);
        }
        .badge {
            padding: 0.25rem 0.75rem;
            border-radius: 9999px;
            font-size: 0.8rem;
            font-weight: 600;
        }
        .badge-success { background: rgba(34, 197, 94, 0.2); color: var(--success); }
        .badge-warning { background: rgba(245, 158, 11, 0.2); color: var(--warning); }
    </style>
</head>
<body>
    <div class="container">
        <div style="display:flex; justify-content:space-between; align-items:center; margin-bottom:1rem;">
            <div>
                <h1>MemoBuild Dashboard</h1>
                <p class="subtitle">Live Build Visualization & Metrics</p>
            </div>
            <div id="live-indicator" style="color:var(--text-dim); font-size:0.9rem;">
                ● Disconnected
            </div>
        </div>
        
        <div class="grid" id="stats">
            <div class="card">
                <div class="stat-label">Total Builds</div>
                <div class="stat-value" id="total-builds">-</div>
            </div>
            <div class="card">
                <div class="stat-label">Cache Hit Rate</div>
                <div class="stat-value" id="hit-rate">-</div>
            </div>
            <div class="card">
                <div class="stat-label">Deduplicated</div>
                <div class="stat-value" id="dedup-ratio">-</div>
            </div>
            <div class="card">
                <div class="stat-label">Active Nodes</div>
                <div class="stat-value" id="active-nodes">0</div>
            </div>
        </div>

        <div id="dag-container"></div>

        <table>
            <thead>
                <tr>
                    <th>Timestamp</th>
                    <th>Dirty</th>
                    <th>Cached</th>
                    <th>Duration</th>
                    <th>Status</th>
                </tr>
            </thead>
            <tbody id="build-list"></tbody>
        </table>
    </div>

    <script>
        let network = null;
        let nodesDS = new vis.DataSet();
        let edgesDS = new vis.DataSet();
        let nodeMap = {};

        async function initDAG() {
            const resp = await fetch('/dag');
            if (!resp.ok) return;
            const dag = await resp.json();
            
            nodesDS.clear();
            edgesDS.clear();
            nodeMap = {};

            dag.nodes.forEach((node, idx) => {
                const color = node.dirty ? '#94a3b8' : '#22c55e';
                nodesDS.add({
                    id: idx,
                    label: node.name,
                    color: { background: color, border: '#1e293b' },
                    font: { color: '#f1f5f9' },
                    shape: 'box',
                    margin: 10
                });
                nodeMap[node.name] = idx;
            });

            dag.edges.forEach(edge => {
                edgesDS.add({ from: edge[0], to: edge[1], arrows: 'to', color: '#334155' });
            });

            const container = document.getElementById('dag-container');
            const data = { nodes: nodesDS, edges: edgesDS };
            const options = {
                layout: { hierarchical: { direction: 'LR', sortMethod: 'directed' } },
                physics: false
            };
            network = new vis.Network(container, data, options);
        }

        function connectWS() {
            const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
            const ws = new WebSocket(`${protocol}//${window.location.host}/ws`);
            const indicator = document.getElementById('live-indicator');

            ws.onopen = () => {
                indicator.innerHTML = '● <span style="color:var(--success)">Live</span>';
                initDAG();
            };

            ws.onclose = () => {
                indicator.innerHTML = '● Disconnected';
                setTimeout(connectWS, 2000);
            };

            ws.onmessage = (e) => {
                const event = JSON.parse(e.data);
                handleBuildEvent(event);
            };
        }

        function handleBuildEvent(event) {
            console.log("Event:", event);
            if (event.BuildStarted) {
                document.getElementById('active-nodes').textContent = event.BuildStarted.total_nodes;
            } else if (event.NodeStarted) {
                nodesDS.update({ id: event.NodeStarted.node_id, color: { background: '#818cf8' } });
            } else if (event.NodeCompleted) {
                const color = event.NodeCompleted.cache_hit ? '#22c55e' : '#38bdf8';
                nodesDS.update({ id: event.NodeCompleted.node_id, color: { background: color } });
                let active = parseInt(document.getElementById('active-nodes').textContent);
                document.getElementById('active-nodes').textContent = Math.max(0, active - 1);
            } else if (event.NodeFailed) {
                nodesDS.update({ id: event.NodeFailed.node_id, color: { background: '#ef4444' } });
            }
        }

        async function loadStats() {
            const [analyticsResp, layerResp] = await Promise.all([
                fetch('/api/analytics'),
                fetch('/api/layers')
            ]);
            
            const data = await analyticsResp.json();
            const layers = await layerResp.json();
            
            if (data.length > 0) {
                document.getElementById('total-builds').textContent = data.length;
                const totalNodes = data.reduce((acc, b) => acc + b.dirty_nodes + b.cached_nodes, 0);
                const totalCached = data.reduce((acc, b) => acc + b.cached_nodes, 0);
                document.getElementById('hit-rate').textContent = totalNodes > 0 ? Math.round((totalCached / totalNodes) * 100) + '%' : '0%';

                const list = document.getElementById('build-list');
                list.innerHTML = data.map(b => `
                    <tr>
                        <td>${new Date(b.timestamp).toLocaleString()}</td>
                        <td>${b.dirty_nodes}</td>
                        <td>${b.cached_nodes}</td>
                        <td>${b.duration_ms}ms</td>
                        <td><span class="badge ${b.dirty_nodes === 0 ? 'badge-success' : 'badge-warning'}">
                            ${b.dirty_nodes === 0 ? 'Pristine' : 'Partial'}
                        </span></td>
                    </tr>
                `).join('');
            }

            if (layers) {
                const ratio = layers.total_size > 0 ? (layers.deduplicated_size / layers.total_size * 100).toFixed(1) : 0;
                document.getElementById('dedup-ratio').textContent = ratio + '%';
            }
        }

        loadStats();
        connectWS();
        setInterval(loadStats, 5000);
    </script>
</body>
</html>
    "#;
    Html(html.to_string())
}

async fn check_cache(
    Path(hash): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match state.metadata.exists(&hash) {
        Ok(true) => {
            let _ = state.metadata.touch(&hash);
            StatusCode::OK
        }
        Ok(false) => StatusCode::NOT_FOUND,
        Err(e) => {
            eprintln!("Error checking cache: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

async fn get_artifact(
    Path(hash): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match state.storage.get(&hash) {
        Ok(Some(data)) => {
            let _ = state.metadata.touch(&hash);
            (StatusCode::OK, data).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            eprintln!("Error getting artifact: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn put_artifact(
    Path(hash): Path<String>,
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> impl IntoResponse {
    // 1. CAS Verification: Verify hash of the body matches requested hash
    let mut hasher = blake3::Hasher::new();
    hasher.update(&body);
    let actual_hash = hasher.finalize().to_hex().to_string();

    if actual_hash != hash {
        let err = crate::error::MemoBuildError::CASIntegrityFailure {
            expected: hash.clone(),
            actual: actual_hash.clone(),
            data_size: body.len(),
        };
        eprintln!("❌ {}", err);
        return StatusCode::BAD_REQUEST;
    }

    let size = body.len() as u64;

    // 2. Store the blob
    match state.storage.put(&hash, &body) {
        Ok(path) => {
            // 3. Update metadata
            if let Err(e) = state.metadata.insert(&hash, &path, size) {
                eprintln!("Error updating metadata: {}", e);
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
            StatusCode::CREATED
        }
        Err(e) => {
            eprintln!("Error storing artifact: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

async fn gc_cache(
    State(state): State<Arc<AppState>>,
    Query(query): Query<GcQuery>,
) -> impl IntoResponse {
    println!(
        "🧹 Running Garbage Collection for entries older than {} days",
        query.days
    );

    match state.metadata.get_old_entries(query.days) {
        Ok(hashes) => {
            let mut node_count = 0;
            for hash in hashes {
                if let Ok(Some(_entry)) = state.metadata.get(&hash) {
                    let _ = state.storage.delete(&hash);
                    let _ = state.metadata.delete(&hash);
                    node_count += 1;
                }
            }

            // Also clean up unused layers
            let mut layer_count = 0;
            if let Ok(unused_layers) = state.metadata.get_unused_layers() {
                for (hash, _path) in unused_layers {
                    let _ = state.storage.delete(&hash);
                    let _ = state.metadata.delete_layer_metadata(&hash);
                    layer_count += 1;
                }
            }

            (
                StatusCode::OK,
                format!(
                    "Deleted {} old artifacts and {} unused layers",
                    node_count, layer_count
                ),
            )
        }
        Err(e) => {
            eprintln!("GC error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        }
    }
}

async fn gc_status() -> impl IntoResponse {
    let gc = crate::gc::GarbageCollector::from_env();
    let status = gc.status().await;
    (StatusCode::OK, Json(status)).into_response()
}

async fn metrics_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let metrics = state.metrics.read().await;
    let output = metrics.encode();
    (
        StatusCode::OK,
        axum::response::AppendHeaders([(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")]),
        output,
    ).into_response()
}

async fn check_layer(
    Path(hash): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match state.metadata.layer_exists(&hash) {
        Ok(true) => StatusCode::OK,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(e) => {
            eprintln!("Error checking layer: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

async fn get_layer(
    Path(hash): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match state.storage.get(&hash) {
        Ok(Some(data)) => (StatusCode::OK, data).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            eprintln!("Error getting layer: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn put_layer(
    Path(hash): Path<String>,
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> impl IntoResponse {
    // CAS Verification: Strict enforcement for layer integrity
    let mut hasher = blake3::Hasher::new();
    hasher.update(&body);
    let actual_hash = hasher.finalize().to_hex().to_string();

    if actual_hash != hash {
        let err = crate::error::MemoBuildError::CASIntegrityFailure {
            expected: hash.clone(),
            actual: actual_hash.clone(),
            data_size: body.len(),
        };
        eprintln!("❌ {}", err);
        return StatusCode::BAD_REQUEST;
    }

    let size = body.len() as u64;
    match state.storage.put(&hash, &body) {
        Ok(path) => {
            if let Err(e) = state.metadata.insert_layer(&hash, &path, size) {
                eprintln!("Error updating layer metadata: {}", e);
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
            StatusCode::CREATED
        }
        Err(e) => {
            eprintln!("Error storing layer: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

#[derive(Deserialize)]
pub struct RegisterLayersRequest {
    pub layers: Vec<String>,
    pub total_size: u64,
}

async fn register_node_layers(
    Path(hash): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RegisterLayersRequest>,
) -> impl IntoResponse {
    match state
        .metadata
        .insert_layered_node(&hash, payload.total_size, &payload.layers)
    {
        Ok(_) => StatusCode::OK,
        Err(e) => {
            eprintln!("Error registering node layers: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

async fn get_node_layers(
    Path(hash): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match state.metadata.get_node_layers(&hash) {
        Ok(Some(layers)) => (StatusCode::OK, Json(layers)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            eprintln!("Error getting node layers: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn report_analytics(
    State(state): State<Arc<AppState>>,
    axum::Json(data): axum::Json<AnalyticsData>,
) -> impl IntoResponse {
    let result = state
        .metadata
        .record_build(data.dirty, data.cached, data.duration_ms);

    // Send build notification if webhook is configured
    if let Some(webhook_url) = state.webhook_url.clone() {
        let stats = data.clone();
        tokio::spawn(async move {
            let client = reqwest::Client::new();
            let payload = serde_json::json!({
                "text": format!(
                    "🚀 *Build Completed*\nNodes: {} dirty, {} cached\nDuration: {}ms\nStatus: {}",
                    stats.dirty,
                    stats.cached,
                    stats.duration_ms,
                    if stats.dirty == 0 { "✅ Pristine" } else { "🔧 Partial" }
                )
            });

            if let Err(e) = client.post(&webhook_url).json(&payload).send().await {
                eprintln!("⚠️ Failed to send build notification: {}", e);
            } else {
                println!("🔔 Build notification sent to webhook");
            }
        });
    }

    match result {
        Ok(_) => StatusCode::OK,
        Err(e) => {
            eprintln!("Analytics error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
