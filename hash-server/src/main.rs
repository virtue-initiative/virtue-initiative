use hash_server::config::Config;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let config = Config::from_env();
    let bind_addr = config.bind_addr.clone();
    let app = hash_server::router(hash_server::init(&config));

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {bind_addr}: {e}"));

    tracing::info!("listening on {bind_addr}");
    axum::serve(listener, app).await.expect("server error");
}
