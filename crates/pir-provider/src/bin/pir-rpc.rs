use std::net::SocketAddr;
use std::sync::Arc;

use axum::{Json, Router, extract::State, routing::post};
use clap::Parser;
use kohaku_pir_provider::{PirProviderConfig, PirRouter};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Parser)]
#[command(
    name = "pir-rpc",
    about = "JSON-RPC 2.0 sidecar over the PIR hybrid router"
)]
struct Args {
    /// PIR serving front, e.g. http://127.0.0.1:8080
    #[arg(long)]
    pir_url: String,

    /// Ordinary Ethereum JSON-RPC URL for methods PIR cannot serve
    #[arg(long)]
    rpc_url: String,

    /// Listen address
    #[arg(long, default_value = "127.0.0.1:8546")]
    listen: String,
}

#[derive(Clone)]
struct App {
    router: Arc<PirRouter>,
}

#[derive(Deserialize)]
struct RpcRequest {
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let listen: SocketAddr = args
        .listen
        .parse()
        .unwrap_or_else(|e| panic!("invalid --listen: {e}"));
    let router = PirRouter::connect(PirProviderConfig {
        pir_url: args.pir_url,
        rpc_url: args.rpc_url,
    })
    .await
    .unwrap_or_else(|e| panic!("failed to connect PIR/fallback: {e}"));
    eprintln!("pir-rpc listening on {listen}");
    let app = Router::new().route("/", post(handle)).with_state(App {
        router: Arc::new(router),
    });
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .unwrap_or_else(|e| panic!("bind {listen}: {e}"));
    axum::serve(listener, app)
        .await
        .unwrap_or_else(|e| panic!("serve: {e}"));
}

async fn handle(State(app): State<App>, Json(body): Json<Value>) -> Json<Value> {
    if let Some(items) = body.as_array() {
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            out.push(dispatch(&app, item.clone()).await);
        }
        return Json(Value::Array(out));
    }
    Json(dispatch(&app, body).await)
}

async fn dispatch(app: &App, body: Value) -> Value {
    let req: RpcRequest = match serde_json::from_value(body) {
        Ok(r) => r,
        Err(e) => {
            return json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": { "code": -32600, "message": e.to_string() }
            });
        }
    };
    match app.router.request(&req.method, req.params).await {
        Ok(result) => json!({
            "jsonrpc": "2.0",
            "id": req.id,
            "result": result
        }),
        Err(e) => json!({
            "jsonrpc": "2.0",
            "id": req.id,
            "error": { "code": e.rpc_code(), "message": e.to_string() }
        }),
    }
}
