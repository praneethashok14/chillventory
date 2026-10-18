use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;

use chillventory::accounts::AccountsState;
use chillventory::routes::{self, AppState};
use chillventory::static_files;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("chillventory: fatal error: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = std::env::var("CHILLVENTORY_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data"));
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(data_dir.join("accounts"))?;

    let accounts = Arc::new(AccountsState::load(data_dir.join("accounts.json"))?);

    let state = AppState::new(accounts, data_dir.clone());
    let api = routes::router(state);
    let app: Router = static_files::attach(api);

    let bind = std::env::var("CHILLVENTORY_BIND").unwrap_or_else(|_| "127.0.0.1:8787".to_string());
    let addr: SocketAddr = bind.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    println!("chillventory listening on http://{bound}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
