mod common;

use common::{signup, TestServer};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::time::Duration;

use fridgeflow::accounts::CSRF_HEADER;

async fn create_appliance(server: &TestServer, csrf: &str, name: &str) -> String {
    let resp = server
        .client
        .post(server.url("/api/appliances"))
        .header(CSRF_HEADER, csrf)
        .json(&json!({ "name": name, "kind": "fridge" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "{:?}", resp.text().await);
    let body: Value = resp.json().await.unwrap();
    body["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn health_is_public() {
    let server = TestServer::start().await;
    let resp = server
        .client
        .get(server.url("/api/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn private_endpoints_require_auth() {
    let server = TestServer::start().await;
    for path in ["/api/appliances", "/api/items", "/api/groups"] {
        let resp = server.client.get(server.url(path)).send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn signup_login_and_crud() {
    let server = TestServer::start().await;
    let csrf = signup(&server, "Pranee", "4242").await;
    let appliance_id = create_appliance(&server, &csrf, "Kitchen Fridge").await;

    // Create an individual item.
    let resp = server
        .client
        .post(server.url(&format!("/api/appliances/{appliance_id}/items")))
        .header(CSRF_HEADER, &csrf)
        .json(&json!({
            "name": "Milk",
            "tracking": "individual",
            "quantity": 1.5,
            "unit": "L",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let item: Value = resp.json().await.unwrap();
    let item_id = item["id"].as_str().unwrap().to_string();

    // Adjust.
    let resp = server
        .client
        .post(server.url(&format!(
            "/api/appliances/{appliance_id}/items/{item_id}/adjust"
        )))
        .header(CSRF_HEADER, &csrf)
        .json(&json!({ "delta": -0.5 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let updated: Value = resp.json().await.unwrap();
    assert_eq!(updated["quantity"].as_f64().unwrap(), 1.0);
}

#[tokio::test]
async fn two_accounts_are_isolated() {
    let server = TestServer::start().await;

    // Alice signs up and creates an appliance.
    let alice_csrf = signup(&server, "Alice", "4242").await;
    create_appliance(&server, &alice_csrf, "Alice Fridge").await;

    // Verify Alice sees her appliance.
    let items: Value = server
        .client
        .get(server.url("/api/items"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(items["appliances"].as_array().unwrap().len(), 1);
    assert_eq!(
        items["appliances"][0]["name"].as_str().unwrap(),
        "Alice Fridge"
    );

    // Alice locks.
    server
        .client
        .post(server.url("/api/auth/lock"))
        .send()
        .await
        .unwrap();

    // Bob signs up with the same PIN as Alice — allowed because names differ.
    let bob_resp = server
        .client
        .post(server.url("/api/auth/signup"))
        .json(&json!({ "name": "Bob", "pin": "4242" }))
        .send()
        .await
        .unwrap();
    assert_eq!(bob_resp.status(), 201);
    let bob_csrf = server
        .client
        .get(server.url("/api/auth/session"))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap()["csrf_token"]
        .as_str()
        .unwrap()
        .to_string();

    // Bob sees no appliances — Alice's data is isolated.
    let items: Value = server
        .client
        .get(server.url("/api/items"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(items["appliances"].as_array().unwrap().len(), 0);

    // Bob creates his own appliance.
    create_appliance(&server, &bob_csrf, "Bob Fridge").await;

    // Alice logs in again (fresh client to clear Bob's cookie).
    let alice = server.new_jar();
    alice
        .post(server.url("/api/auth/login"))
        .json(&json!({ "name": "Alice", "pin": "4242" }))
        .send()
        .await
        .unwrap();
    let alice_items: Value = alice
        .get(server.url("/api/items"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let alice_names: Vec<&str> = alice_items["appliances"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert_eq!(alice_names, vec!["Alice Fridge"]);
}

#[tokio::test]
async fn duplicate_name_rejected() {
    let server = TestServer::start().await;
    signup(&server, "Pranee", "4242").await;
    let resp = server
        .new_jar()
        .post(server.url("/api/auth/signup"))
        .json(&json!({ "name": "pranee", "pin": "1111" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn wrong_name_or_pin_returns_401() {
    let server = TestServer::start().await;
    signup(&server, "Pranee", "4242").await;
    let fresh = server.new_jar();
    for body in [
        json!({ "name": "Pranee", "pin": "0000" }),
        json!({ "name": "Nobody", "pin": "4242" }),
    ] {
        let resp = fresh
            .post(server.url("/api/auth/login"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{body}");
    }
}

#[tokio::test]
async fn pack_open_conserves_total() {
    let server = TestServer::start().await;
    let csrf = signup(&server, "Pranee", "778899").await;
    let appliance_id = create_appliance(&server, &csrf, "Freezer").await;

    let group: Value = server
        .client
        .post(server.url("/api/groups"))
        .header(CSRF_HEADER, &csrf)
        .json(&json!({ "name": "Peas", "appliance_id": appliance_id }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let group_id = group["id"].as_str().unwrap().to_string();

    let unopened: Value = server
        .client
        .post(server.url(&format!("/api/appliances/{appliance_id}/items")))
        .header(CSRF_HEADER, &csrf)
        .json(&json!({
            "name": "Peas",
            "group_id": group_id,
            "tracking": "pack",
            "pack_count": 4,
            "pack_label": "bag",
            "pack_status": "unopened",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let unopened_id = unopened["id"].as_str().unwrap().to_string();

    let one: Value = server
        .client
        .post(server.url(&format!(
            "/api/appliances/{appliance_id}/items/{unopened_id}/open-one"
        )))
        .header(CSRF_HEADER, &csrf)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(one["unopened"]["pack_count"].as_u64().unwrap(), 3);
    assert_eq!(one["opened"]["pack_count"].as_u64().unwrap(), 1);

    let all: Value = server
        .client
        .post(server.url(&format!(
            "/api/appliances/{appliance_id}/items/{unopened_id}/open-all"
        )))
        .header(CSRF_HEADER, &csrf)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(all["unopened"]["pack_count"].as_u64().unwrap(), 0);
    assert_eq!(all["opened"]["pack_count"].as_u64().unwrap(), 4);
}

#[tokio::test]
async fn concurrent_adjustments_do_not_lose_updates() {
    let server = TestServer::start().await;
    let csrf = signup(&server, "Pranee", "246810").await;
    let appliance_id = create_appliance(&server, &csrf, "Pantry").await;

    let item: Value = server
        .client
        .post(server.url(&format!("/api/appliances/{appliance_id}/items")))
        .header(CSRF_HEADER, &csrf)
        .json(&json!({
            "name": "Flour",
            "tracking": "individual",
            "quantity": 0.0,
            "unit": "kg",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let item_id = item["id"].as_str().unwrap().to_string();

    let mut handles = Vec::new();
    for _ in 0..20 {
        let client = server.client.clone();
        let url = server.url(&format!(
            "/api/appliances/{appliance_id}/items/{item_id}/adjust"
        ));
        let csrf = csrf.clone();
        handles.push(tokio::spawn(async move {
            client
                .post(url)
                .header(CSRF_HEADER, csrf)
                .json(&json!({ "delta": 1.0 }))
                .send()
                .await
                .unwrap()
                .status()
        }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap(), 200);
    }

    let items: Value = server
        .client
        .get(server.url("/api/items"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let item = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == item_id)
        .unwrap();
    assert_eq!(item["quantity"].as_f64().unwrap(), 20.0);
}

#[tokio::test]
async fn session_expiry_rejects_stale_cookie() {
    let server = TestServer::start_with_ttl(Duration::from_millis(50)).await;
    signup(&server, "Pranee", "555555").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let resp = server
        .client
        .get(server.url("/api/appliances"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn lock_revokes_session() {
    let server = TestServer::start().await;
    signup(&server, "Pranee", "909090").await;
    let resp = server
        .client
        .post(server.url("/api/auth/lock"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let resp = server
        .client
        .get(server.url("/api/appliances"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn csrf_header_required_for_mutations() {
    let server = TestServer::start().await;
    signup(&server, "Pranee", "121212").await;
    let resp = server
        .client
        .post(server.url("/api/appliances"))
        .json(&json!({ "name": "x", "kind": "fridge" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn delete_appliance_with_contents_requires_strategy() {
    let server = TestServer::start().await;
    let csrf = signup(&server, "Pranee", "131313").await;
    let appliance_id = create_appliance(&server, &csrf, "Main").await;
    server
        .client
        .post(server.url(&format!("/api/appliances/{appliance_id}/items")))
        .header(CSRF_HEADER, &csrf)
        .json(&json!({
            "name": "Eggs",
            "tracking": "individual",
            "quantity": 12.0,
            "unit": "pcs",
        }))
        .send()
        .await
        .unwrap();

    let resp = server
        .client
        .delete(server.url(&format!("/api/appliances/{appliance_id}")))
        .header(CSRF_HEADER, &csrf)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let resp = server
        .client
        .delete(server.url(&format!("/api/appliances/{appliance_id}?strategy=cascade")))
        .header(CSRF_HEADER, &csrf)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn change_pin_revokes_sessions() {
    let server = TestServer::start().await;
    let csrf = signup(&server, "Pranee", "414141").await;
    let resp = server
        .client
        .post(server.url("/api/auth/change-pin"))
        .header(CSRF_HEADER, &csrf)
        .json(&json!({ "current_pin": "000000", "new_pin": "515151" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let resp = server
        .client
        .post(server.url("/api/auth/change-pin"))
        .header(CSRF_HEADER, &csrf)
        .json(&json!({ "current_pin": "414141", "new_pin": "515151" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let resp = server
        .client
        .get(server.url("/api/appliances"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let fresh = server.new_jar();
    let resp = fresh
        .post(server.url("/api/auth/login"))
        .json(&json!({ "name": "Pranee", "pin": "515151" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn persistence_across_restart() {
    use fridgeflow::accounts::AccountsState;
    use fridgeflow::routes::{self, AppState};
    use std::net::SocketAddr;
    use std::sync::Arc;

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("accounts")).unwrap();

    // Boot once.
    {
        let accounts = Arc::new(AccountsState::load(dir.path().join("accounts.json")).unwrap());
        let state = AppState::new(accounts, dir.path().to_path_buf());
        let app = fridgeflow::static_files::attach(routes::router(state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        let base = format!("http://{addr}");
        let c = reqwest::Client::builder()
            .cookie_store(true)
            .build()
            .unwrap();
        c.post(format!("{base}/api/auth/signup"))
            .json(&json!({ "name": "Pranee", "pin": "909192" }))
            .send()
            .await
            .unwrap();
        let csrf = c
            .get(format!("{base}/api/auth/session"))
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()["csrf_token"]
            .as_str()
            .unwrap()
            .to_string();
        c.post(format!("{base}/api/appliances"))
            .header(CSRF_HEADER, &csrf)
            .json(&json!({ "name": "Persisted Fridge", "kind": "fridge" }))
            .send()
            .await
            .unwrap();
        server.abort();
    }

    // Boot again against the same data dir.
    let accounts = Arc::new(AccountsState::load(dir.path().join("accounts.json")).unwrap());
    let state = AppState::new(accounts, dir.path().to_path_buf());
    let app = fridgeflow::static_files::attach(routes::router(state));
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
    let base = format!("http://{addr}");
    let c = reqwest::Client::builder()
        .cookie_store(true)
        .build()
        .unwrap();
    let resp = c
        .post(format!("{base}/api/auth/login"))
        .json(&json!({ "name": "Pranee", "pin": "909192" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let items: Value = c
        .get(format!("{base}/api/items"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let names: Vec<&str> = items["appliances"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Persisted Fridge"]);
}

#[tokio::test]
async fn auth_status_lists_account_names() {
    let server = TestServer::start().await;
    signup(&server, "Alice", "4242").await;
    server
        .client
        .post(server.url("/api/auth/lock"))
        .send()
        .await
        .unwrap();
    let fresh = server.new_jar();
    fresh
        .post(server.url("/api/auth/signup"))
        .json(&json!({ "name": "Bob", "pin": "1111" }))
        .send()
        .await
        .unwrap();
    let status: Value = server
        .client
        .get(server.url("/api/auth/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["has_accounts"], true);
    assert_eq!(status["account_count"], 2);
    let names: Vec<&str> = status["account_names"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap())
        .collect();
    assert!(names.contains(&"Alice"));
    assert!(names.contains(&"Bob"));
}
