use std::net::SocketAddr;
use std::sync::Arc;

use stockspace::accounts::AccountsState;
use stockspace::routes::{self, AppState};
use tempfile::TempDir;

pub struct TestServer {
    pub addr: SocketAddr,
    pub client: reqwest::Client,
    #[allow(dead_code)]
    pub dir: TempDir,
}

impl TestServer {
    pub async fn start() -> Self {
        Self::start_with_ttl(std::time::Duration::from_secs(60 * 60)).await
    }

    pub async fn start_with_ttl(ttl: std::time::Duration) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("accounts")).unwrap();
        let accounts = Arc::new(
            AccountsState::load(dir.path().join("accounts.json"))
                .unwrap()
                .with_session_ttl(ttl),
        );
        let state = AppState::new(accounts, dir.path().to_path_buf());
        let api = routes::router(state);
        let app = stockspace::static_files::attach(api);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        let client = reqwest::Client::builder()
            .cookie_store(true)
            .build()
            .unwrap();
        Self { addr, client, dir }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    pub fn new_jar(&self) -> reqwest::Client {
        reqwest::Client::builder()
            .cookie_store(true)
            .build()
            .unwrap()
    }
}

pub async fn signup(server: &TestServer, name: &str, pin: &str) -> String {
    let resp = server
        .client
        .post(server.url("/api/auth/signup"))
        .json(&serde_json::json!({ "name": name, "pin": pin }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "signup failed: {:?}", resp.text().await);
    let s: serde_json::Value = server
        .client
        .get(server.url("/api/auth/session"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    s.get("csrf_token")
        .and_then(|v| v.as_str())
        .expect("csrf token after signup")
        .to_string()
}
