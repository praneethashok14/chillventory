use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;

use crate::accounts::{
    constant_time_eq, AccountsState, AuthError, SessionInfo, CSRF_HEADER, SESSION_COOKIE,
};
use crate::models::{
    is_finite_nonneg, is_valid_iso_date, is_valid_name, new_id, now, Appliance, ApplianceKind,
    Group, Inventory, InventoryItem, ItemTracking, PackStatus,
};
use crate::storage::{InventoryStore, StorageError};

#[derive(Clone)]
pub struct AppState {
    pub accounts: Arc<AccountsState>,
    pub data_dir: PathBuf,
    inventories: Arc<Mutex<HashMap<String, Arc<InventoryStore>>>>,
}

impl AppState {
    pub fn new(accounts: Arc<AccountsState>, data_dir: PathBuf) -> Self {
        Self {
            accounts,
            data_dir,
            inventories: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn inventory(&self, account_id: &str) -> Result<Arc<InventoryStore>, StorageError> {
        {
            let map = self.inventories.lock().unwrap();
            if let Some(inv) = map.get(account_id) {
                return Ok(inv.clone());
            }
        }
        let path = self
            .data_dir
            .join("accounts")
            .join(account_id)
            .join("inventory.json");
        let store = Arc::new(InventoryStore::load_or_init(path)?);
        let mut map = self.inventories.lock().unwrap();
        Ok(map
            .entry(account_id.to_string())
            .or_insert_with(|| store)
            .clone())
    }

    /// Resolve the photo file path for an item inside an account. Also
    /// guards against path traversal: `item_id` must look like a UUID so we
    /// never let a request escape the per-account photos directory.
    pub fn photo_path(&self, account_id: &str, item_id: &str) -> Option<PathBuf> {
        if !is_safe_id(item_id) || !is_safe_id(account_id) {
            return None;
        }
        Some(
            self.data_dir
                .join("accounts")
                .join(account_id)
                .join("photos")
                .join(format!("{item_id}.jpg")),
        )
    }
}

fn is_safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[derive(Debug)]
pub enum ApiError {
    NotFound(String),
    BadRequest(String),
    Conflict(String),
    Unauthorized,
    Forbidden,
    RateLimited(u64),
    Internal(String),
}

impl From<StorageError> for ApiError {
    fn from(value: StorageError) -> Self {
        ApiError::Internal(value.to_string())
    }
}

impl From<AuthError> for ApiError {
    fn from(value: AuthError) -> Self {
        match value {
            AuthError::NoAccounts => ApiError::BadRequest("no accounts configured".to_string()),
            AuthError::NameTaken => ApiError::Conflict("that name is already in use".to_string()),
            AuthError::NameTooShort => ApiError::BadRequest("name is required".to_string()),
            AuthError::NameTooLong => ApiError::BadRequest("name is too long".to_string()),
            AuthError::InvalidCredentials => ApiError::Unauthorized,
            AuthError::RateLimited(d) => ApiError::RateLimited(d.as_secs().max(1)),
            AuthError::PinTooWeak => ApiError::BadRequest("pin must be 4-12 digits".to_string()),
            AuthError::Storage(e) => ApiError::Internal(e.to_string()),
            AuthError::Hash(_) => ApiError::Internal("internal authentication error".to_string()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, "not_found", m),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, "bad_request", m),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, "conflict", m),
            ApiError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "authentication required".to_string(),
            ),
            ApiError::Forbidden => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "request not permitted".to_string(),
            ),
            ApiError::RateLimited(secs) => {
                let mut resp = (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({
                        "error": {
                            "code": "rate_limited",
                            "message": "too many attempts",
                            "retry_after_seconds": secs,
                        }
                    })),
                )
                    .into_response();
                resp.headers_mut().insert(
                    "retry-after",
                    HeaderValue::from_str(&secs.to_string()).unwrap(),
                );
                return resp;
            }
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, "internal", m),
        };
        (
            status,
            Json(json!({ "error": { "code": code, "message": message } })),
        )
            .into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/status", get(auth_status))
        .route("/api/auth/signup", post(auth_signup))
        .route("/api/auth/login", post(auth_login))
        .route("/api/auth/lock", post(auth_lock))
        .route("/api/auth/session", get(auth_session))
        .route("/api/auth/change-pin", post(auth_change_pin))
        .route(
            "/api/appliances",
            get(list_appliances).post(create_appliance),
        )
        .route(
            "/api/appliances/:id",
            get(get_appliance)
                .patch(update_appliance)
                .delete(delete_appliance),
        )
        .route(
            "/api/appliances/:id/items",
            get(list_appliance_items).post(create_item),
        )
        .route(
            "/api/appliances/:id/items/:item_id",
            patch(update_item).delete(delete_item),
        )
        .route(
            "/api/appliances/:id/items/:item_id/adjust",
            post(adjust_item),
        )
        .route(
            "/api/appliances/:id/items/:item_id/open-one",
            post(mark_one_open),
        )
        .route(
            "/api/appliances/:id/items/:item_id/open-all",
            post(mark_all_open),
        )
        .route(
            "/api/appliances/:id/items/:item_id/close-one",
            post(mark_one_unopened),
        )
        .route(
            "/api/appliances/:id/items/:item_id/close-all",
            post(mark_all_unopened),
        )
        .route(
            "/api/appliances/:id/items/:item_id/photo",
            get(get_photo).post(upload_photo).delete(delete_photo),
        )
        .route("/api/groups", get(list_groups).post(create_group))
        .route("/api/groups/:id", patch(update_group).delete(delete_group))
        .route("/api/items", get(list_items))
        .with_state(state)
}

// ---------- helpers ----------

fn require_auth(accounts: &AccountsState, headers: &HeaderMap) -> Result<SessionInfo, ApiError> {
    let cookie_value = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let session_id = parse_cookie(cookie_value, SESSION_COOKIE).ok_or(ApiError::Unauthorized)?;
    accounts.session(&session_id).ok_or(ApiError::Unauthorized)
}

fn require_csrf(session: &SessionInfo, headers: &HeaderMap) -> Result<(), ApiError> {
    let token = headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or(ApiError::Forbidden)?;
    if constant_time_eq(token, &session.csrf) {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

fn parse_cookie(header_value: &str, name: &str) -> Option<String> {
    for part in header_value.split(';') {
        let trimmed = part.trim();
        if let Some((k, v)) = trimmed.split_once('=') {
            if k == name {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn session_cookie_header(session: &SessionInfo) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}={}; Path=/; HttpOnly; SameSite=Strict",
        session.id
    ))
    .expect("cookie header is ascii")
}

fn clear_session_cookie_header() -> HeaderValue {
    HeaderValue::from_static("stockspace_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
}

fn session_json(session: &SessionInfo, account_name: Option<String>) -> serde_json::Value {
    json!({
        "authenticated": true,
        "csrf_token": session.csrf,
        "account_id": session.account_id,
        "account_name": account_name,
    })
}

// ---------- health & auth ----------

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

async fn auth_status(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({
        "has_accounts": state.accounts.has_accounts(),
        "account_count": state.accounts.account_count(),
        "account_names": state.accounts.account_names(),
    }))
}

#[derive(Deserialize)]
struct AuthBody {
    name: String,
    pin: String,
}

#[derive(Deserialize)]
struct ChangePinBody {
    current_pin: String,
    new_pin: String,
}

async fn auth_signup(
    State(state): State<AppState>,
    Json(body): Json<AuthBody>,
) -> ApiResult<Response> {
    let session = state.accounts.signup(&body.name, &body.pin)?;
    let name = state.accounts.account(&session.account_id).map(|a| a.name);
    let mut resp = (StatusCode::CREATED, Json(session_json(&session, name))).into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, session_cookie_header(&session));
    Ok(resp)
}

async fn auth_login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Json(body): Json<AuthBody>,
) -> ApiResult<Response> {
    let session = state
        .accounts
        .login(&body.name, &body.pin, Some(addr.ip()))?;
    let name = state.accounts.account(&session.account_id).map(|a| a.name);
    let mut resp = Json(session_json(&session, name)).into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, session_cookie_header(&session));
    Ok(resp)
}

async fn auth_lock(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let cookie_value = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if let Some(id) = parse_cookie(cookie_value, SESSION_COOKIE) {
        state.accounts.revoke(&id);
    }
    let mut resp = StatusCode::NO_CONTENT.into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, clear_session_cookie_header());
    resp
}

async fn auth_session(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    match require_auth(&state.accounts, &headers) {
        Ok(session) => {
            let name = state.accounts.account(&session.account_id).map(|a| a.name);
            (StatusCode::OK, Json(session_json(&session, name))).into_response()
        }
        Err(_) => (StatusCode::OK, Json(json!({ "authenticated": false }))).into_response(),
    }
}

async fn auth_change_pin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ChangePinBody>,
) -> ApiResult<Response> {
    let session = require_auth(&state.accounts, &headers)?;
    require_csrf(&session, &headers)?;
    state
        .accounts
        .change_pin(&session.account_id, &body.current_pin, &body.new_pin)?;
    let mut resp = StatusCode::NO_CONTENT.into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, clear_session_cookie_header());
    Ok(resp)
}

// ---------- appliances ----------

#[derive(Deserialize)]
struct CreateApplianceBody {
    name: String,
    kind: ApplianceKind,
    location: Option<String>,
    description: Option<String>,
}

async fn list_appliances(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    let session = require_auth(&state.accounts, &headers)?;
    let inv = state.inventory(&session.account_id)?;
    let appliances = inv.with_read(|i| i.appliances.clone());
    Ok(Json(json!({ "appliances": appliances })))
}

async fn create_appliance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateApplianceBody>,
) -> ApiResult<(StatusCode, Json<Appliance>)> {
    let session = require_auth(&state.accounts, &headers)?;
    require_csrf(&session, &headers)?;
    if !is_valid_name(&body.name) {
        return Err(ApiError::BadRequest("name is required".to_string()));
    }
    let appliance = Appliance {
        id: new_id(),
        name: body.name.trim().to_string(),
        kind: body.kind,
        location: body.location.and_then(blank_to_none),
        description: body.description.and_then(blank_to_none),
        created_at: now(),
        updated_at: now(),
    };
    let inv = state.inventory(&session.account_id)?;
    let created = inv.mutate(|i| {
        i.appliances.push(appliance.clone());
        Ok(appliance.clone())
    })?;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn get_appliance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let session = require_auth(&state.accounts, &headers)?;
    let inv = state.inventory(&session.account_id)?;
    inv.with_read(|i| {
        let appliance = i
            .appliances
            .iter()
            .find(|a| a.id == id)
            .cloned()
            .ok_or_else(|| ApiError::NotFound(format!("appliance {id} not found")))?;
        let groups: Vec<_> = i
            .groups
            .iter()
            .filter(|g| g.appliance_id == id)
            .cloned()
            .collect();
        let items: Vec<_> = i
            .items
            .iter()
            .filter(|x| x.appliance_id == id)
            .cloned()
            .collect();
        Ok(Json(json!({
            "appliance": appliance,
            "groups": groups,
            "items": items,
        })))
    })
}

#[derive(Deserialize)]
struct UpdateApplianceBody {
    name: Option<String>,
    kind: Option<ApplianceKind>,
    location: Option<Option<String>>,
    description: Option<Option<String>>,
}

async fn update_appliance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateApplianceBody>,
) -> ApiResult<Json<Appliance>> {
    let session = require_auth(&state.accounts, &headers)?;
    require_csrf(&session, &headers)?;
    let inv = state.inventory(&session.account_id)?;
    let updated = inv.mutate(|i| {
        let appliance = i
            .appliances
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| StorageError::Corrupt(format!("appliance {id} not found")))?;
        if let Some(name) = body.name {
            if !is_valid_name(&name) {
                return Err(StorageError::Corrupt("name must not be empty".to_string()));
            }
            appliance.name = name.trim().to_string();
        }
        if let Some(kind) = body.kind {
            appliance.kind = kind;
        }
        if let Some(location) = body.location {
            appliance.location = location.and_then(blank_to_none);
        }
        if let Some(description) = body.description {
            appliance.description = description.and_then(blank_to_none);
        }
        appliance.updated_at = now();
        Ok(appliance.clone())
    });
    updated.map(Json).map_err(storage_err_to_api)
}

#[derive(Deserialize, Default)]
struct DeleteApplianceParams {
    #[serde(default)]
    strategy: Option<String>,
    #[serde(default)]
    move_to: Option<String>,
}

async fn delete_appliance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(params): Query<DeleteApplianceParams>,
) -> ApiResult<StatusCode> {
    let session = require_auth(&state.accounts, &headers)?;
    require_csrf(&session, &headers)?;
    let inv = state.inventory(&session.account_id)?;
    let strategy = params.strategy.as_deref().unwrap_or("");
    let has_contents = inv.with_read(|i| {
        i.items.iter().any(|x| x.appliance_id == id)
            || i.groups.iter().any(|g| g.appliance_id == id)
    });
    if has_contents && strategy != "cascade" && strategy != "move" {
        return Err(ApiError::Conflict(
            "appliance has contents; specify strategy=cascade or strategy=move&move_to=<id>"
                .to_string(),
        ));
    }
    inv.mutate(|i| {
        let exists = i.appliances.iter().any(|a| a.id == id);
        if !exists {
            return Err(StorageError::Corrupt(format!("appliance {id} not found")));
        }
        match strategy {
            "move" => {
                let target = params
                    .move_to
                    .clone()
                    .ok_or_else(|| StorageError::Corrupt("move_to is required".into()))?;
                if target == id {
                    return Err(StorageError::Corrupt(
                        "cannot move contents to the appliance being deleted".into(),
                    ));
                }
                if !i.appliances.iter().any(|a| a.id == target) {
                    return Err(StorageError::Corrupt(format!(
                        "target appliance {target} not found"
                    )));
                }
                for item in i.items.iter_mut() {
                    if item.appliance_id == id {
                        item.appliance_id = target.clone();
                        item.updated_at = now();
                    }
                }
                for group in i.groups.iter_mut() {
                    if group.appliance_id == id {
                        group.appliance_id = target.clone();
                        group.updated_at = now();
                    }
                }
            }
            "cascade" => {
                i.items.retain(|x| x.appliance_id != id);
                i.groups.retain(|g| g.appliance_id != id);
            }
            _ => {}
        }
        i.appliances.retain(|a| a.id != id);
        Ok(())
    })
    .map_err(storage_err_to_api)?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- items ----------

#[derive(Deserialize)]
struct CreateItemBody {
    name: String,
    group_id: Option<String>,
    category: Option<String>,
    #[serde(flatten)]
    tracking: TrackingInput,
    expiration_date: Option<String>,
    notes: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "tracking", rename_all = "snake_case")]
enum TrackingInput {
    Individual {
        quantity: f64,
        unit: String,
    },
    Pack {
        pack_count: u32,
        pack_label: String,
        pack_status: PackStatus,
    },
}

fn validate_tracking(t: &ItemTracking) -> Result<(), ApiError> {
    match t {
        ItemTracking::Individual { quantity, unit } => {
            if !is_finite_nonneg(*quantity) {
                return Err(ApiError::BadRequest(
                    "quantity must be a finite non-negative number".to_string(),
                ));
            }
            if !is_valid_name(unit) {
                return Err(ApiError::BadRequest("unit is required".to_string()));
            }
        }
        ItemTracking::Pack { pack_label, .. } => {
            if !is_valid_name(pack_label) {
                return Err(ApiError::BadRequest("pack_label is required".to_string()));
            }
        }
    }
    Ok(())
}

async fn list_appliance_items(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let session = require_auth(&state.accounts, &headers)?;
    let inv = state.inventory(&session.account_id)?;
    inv.with_read(|i| {
        if !i.appliances.iter().any(|a| a.id == id) {
            return Err(ApiError::NotFound(format!("appliance {id} not found")));
        }
        let items: Vec<_> = i
            .items
            .iter()
            .filter(|x| x.appliance_id == id)
            .cloned()
            .collect();
        Ok(Json(json!({ "items": items })))
    })
}

async fn create_item(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(appliance_id): Path<String>,
    Json(body): Json<CreateItemBody>,
) -> ApiResult<(StatusCode, Json<InventoryItem>)> {
    let session = require_auth(&state.accounts, &headers)?;
    require_csrf(&session, &headers)?;
    if !is_valid_name(&body.name) {
        return Err(ApiError::BadRequest("name is required".to_string()));
    }
    if let Some(ref date) = body.expiration_date {
        if !date.is_empty() && !is_valid_iso_date(date) {
            return Err(ApiError::BadRequest(
                "expiration_date must be ISO-8601 (YYYY-MM-DD)".to_string(),
            ));
        }
    }
    let tracking = match body.tracking {
        TrackingInput::Individual { quantity, unit } => ItemTracking::Individual {
            quantity,
            unit: unit.trim().to_string(),
        },
        TrackingInput::Pack {
            pack_count,
            pack_label,
            pack_status,
        } => ItemTracking::Pack {
            pack_count,
            pack_label: pack_label.trim().to_string(),
            pack_status,
        },
    };
    validate_tracking(&tracking)?;

    let item = InventoryItem {
        id: new_id(),
        name: body.name.trim().to_string(),
        appliance_id: appliance_id.clone(),
        group_id: body.group_id,
        category: body.category.and_then(blank_to_none),
        tracking,
        expiration_date: body.expiration_date.and_then(blank_to_none),
        notes: body.notes.and_then(blank_to_none),
        has_photo: false,
        created_at: now(),
        updated_at: now(),
    };
    let inv = state.inventory(&session.account_id)?;
    let created = inv
        .mutate(|i| {
            if !i.appliances.iter().any(|a| a.id == appliance_id) {
                return Err(StorageError::Corrupt(format!(
                    "appliance {appliance_id} not found"
                )));
            }
            if let Some(ref gid) = item.group_id {
                let gref = i
                    .groups
                    .iter()
                    .find(|g| &g.id == gid)
                    .ok_or_else(|| StorageError::Corrupt(format!("group {gid} not found")))?;
                if gref.appliance_id != appliance_id {
                    return Err(StorageError::Corrupt(
                        "group belongs to a different appliance".into(),
                    ));
                }
            }
            i.items.push(item.clone());
            Ok(item.clone())
        })
        .map_err(storage_err_to_api)?;
    Ok((StatusCode::CREATED, Json(created)))
}

#[derive(Deserialize)]
struct UpdateItemBody {
    name: Option<String>,
    group_id: Option<Option<String>>,
    category: Option<Option<String>>,
    expiration_date: Option<Option<String>>,
    notes: Option<Option<String>>,
    pack_label: Option<String>,
    unit: Option<String>,
}

async fn update_item(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((appliance_id, item_id)): Path<(String, String)>,
    Json(body): Json<UpdateItemBody>,
) -> ApiResult<Json<InventoryItem>> {
    let session = require_auth(&state.accounts, &headers)?;
    require_csrf(&session, &headers)?;
    if let Some(ref date_opt) = body.expiration_date {
        if let Some(ref date) = date_opt {
            if !date.is_empty() && !is_valid_iso_date(date) {
                return Err(ApiError::BadRequest(
                    "expiration_date must be ISO-8601 (YYYY-MM-DD)".to_string(),
                ));
            }
        }
    }
    let inv = state.inventory(&session.account_id)?;
    let updated = inv.mutate(|i| {
        let item = i
            .items
            .iter_mut()
            .find(|x| x.id == item_id && x.appliance_id == appliance_id)
            .ok_or_else(|| StorageError::Corrupt(format!("item {item_id} not found")))?;
        if let Some(name) = body.name {
            if !is_valid_name(&name) {
                return Err(StorageError::Corrupt("name must not be empty".into()));
            }
            item.name = name.trim().to_string();
        }
        if let Some(group_id) = body.group_id {
            item.group_id = group_id.and_then(blank_to_none);
        }
        if let Some(category) = body.category {
            item.category = category.and_then(blank_to_none);
        }
        if let Some(expiration_date) = body.expiration_date {
            item.expiration_date = expiration_date.and_then(blank_to_none);
        }
        if let Some(notes) = body.notes {
            item.notes = notes.and_then(blank_to_none);
        }
        match &mut item.tracking {
            ItemTracking::Individual { unit, .. } => {
                if let Some(new_unit) = body.unit {
                    if !is_valid_name(&new_unit) {
                        return Err(StorageError::Corrupt("unit must not be empty".into()));
                    }
                    *unit = new_unit.trim().to_string();
                }
            }
            ItemTracking::Pack { pack_label, .. } => {
                if let Some(new_label) = body.pack_label {
                    if !is_valid_name(&new_label) {
                        return Err(StorageError::Corrupt("pack_label must not be empty".into()));
                    }
                    *pack_label = new_label.trim().to_string();
                }
            }
        }
        if let Some(ref gid) = item.group_id {
            let belongs = i
                .groups
                .iter()
                .any(|g| &g.id == gid && g.appliance_id == appliance_id);
            if !belongs {
                return Err(StorageError::Corrupt(
                    "group does not exist in this appliance".into(),
                ));
            }
        }
        let item = i
            .items
            .iter_mut()
            .find(|x| x.id == item_id)
            .expect("item still exists");
        item.updated_at = now();
        Ok(item.clone())
    });
    updated.map(Json).map_err(storage_err_to_api)
}

async fn delete_item(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((appliance_id, item_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let session = require_auth(&state.accounts, &headers)?;
    require_csrf(&session, &headers)?;
    let inv = state.inventory(&session.account_id)?;
    inv.mutate(|i| {
        let before = i.items.len();
        i.items
            .retain(|x| !(x.id == item_id && x.appliance_id == appliance_id));
        if i.items.len() == before {
            return Err(StorageError::Corrupt(format!("item {item_id} not found")));
        }
        Ok(())
    })
    .map_err(storage_err_to_api)?;
    // Best-effort photo cleanup. Not a hard error if the file is missing or
    // can't be removed — the item is already gone from the inventory record.
    if let Some(path) = state.photo_path(&session.account_id, &item_id) {
        let _ = std::fs::remove_file(path);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct AdjustBody {
    delta: f64,
}

async fn adjust_item(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((appliance_id, item_id)): Path<(String, String)>,
    Json(body): Json<AdjustBody>,
) -> ApiResult<Json<InventoryItem>> {
    let session = require_auth(&state.accounts, &headers)?;
    require_csrf(&session, &headers)?;
    if !body.delta.is_finite() {
        return Err(ApiError::BadRequest("delta must be a finite number".into()));
    }
    let inv = state.inventory(&session.account_id)?;
    let updated = inv.mutate(|i| {
        let item = i
            .items
            .iter_mut()
            .find(|x| x.id == item_id && x.appliance_id == appliance_id)
            .ok_or_else(|| StorageError::Corrupt(format!("item {item_id} not found")))?;
        match &mut item.tracking {
            ItemTracking::Individual { quantity, .. } => {
                let next = *quantity + body.delta;
                if !is_finite_nonneg(next) {
                    return Err(StorageError::Corrupt(
                        "resulting quantity would be invalid".into(),
                    ));
                }
                *quantity = round_quantity(next);
            }
            ItemTracking::Pack { pack_count, .. } => {
                if body.delta.fract() != 0.0 {
                    return Err(StorageError::Corrupt(
                        "pack counts change in whole-pack increments".into(),
                    ));
                }
                let next = *pack_count as i64 + body.delta as i64;
                if next < 0 {
                    return Err(StorageError::Corrupt(
                        "pack count cannot go below zero".into(),
                    ));
                }
                if next > u32::MAX as i64 {
                    return Err(StorageError::Corrupt("pack count overflow".into()));
                }
                *pack_count = next as u32;
            }
        }
        item.updated_at = now();
        Ok(item.clone())
    });
    updated.map(Json).map_err(storage_err_to_api)
}

fn round_quantity(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

// ---------- pack opening / closing ----------

async fn mark_one_open(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((appliance_id, item_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    pack_transfer_handler(
        state,
        headers,
        appliance_id,
        item_id,
        TransferDirection::Open,
        TransferMode::One,
    )
}

async fn mark_all_open(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((appliance_id, item_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    pack_transfer_handler(
        state,
        headers,
        appliance_id,
        item_id,
        TransferDirection::Open,
        TransferMode::All,
    )
}

async fn mark_one_unopened(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((appliance_id, item_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    pack_transfer_handler(
        state,
        headers,
        appliance_id,
        item_id,
        TransferDirection::Close,
        TransferMode::One,
    )
}

async fn mark_all_unopened(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((appliance_id, item_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    pack_transfer_handler(
        state,
        headers,
        appliance_id,
        item_id,
        TransferDirection::Close,
        TransferMode::All,
    )
}

fn pack_transfer_handler(
    state: AppState,
    headers: HeaderMap,
    appliance_id: String,
    item_id: String,
    direction: TransferDirection,
    mode: TransferMode,
) -> ApiResult<Json<serde_json::Value>> {
    let session = require_auth(&state.accounts, &headers)?;
    require_csrf(&session, &headers)?;
    let inv = state.inventory(&session.account_id)?;
    let resp = pack_transfer(&inv, &appliance_id, &item_id, direction, mode)?;
    // If the source item was auto-deleted, best-effort remove its photo.
    if resp.0.get("source_deleted").and_then(|v| v.as_bool()) == Some(true) {
        if let Some(path) = state.photo_path(&session.account_id, &item_id) {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(resp)
}

#[derive(Clone, Copy)]
enum TransferMode {
    One,
    All,
}

#[derive(Clone, Copy)]
enum TransferDirection {
    /// Unopened → Opened.
    Open,
    /// Opened → Unopened.
    Close,
}

impl TransferDirection {
    fn source_status(self) -> PackStatus {
        match self {
            TransferDirection::Open => PackStatus::Unopened,
            TransferDirection::Close => PackStatus::Opened,
        }
    }
    fn dest_status(self) -> PackStatus {
        match self {
            TransferDirection::Open => PackStatus::Opened,
            TransferDirection::Close => PackStatus::Unopened,
        }
    }
}

fn pack_transfer(
    store: &InventoryStore,
    appliance_id: &str,
    item_id: &str,
    direction: TransferDirection,
    mode: TransferMode,
) -> ApiResult<Json<serde_json::Value>> {
    let (source_after, dest_after) = store
        .mutate(|i| perform_transfer(i, appliance_id, item_id, direction, mode))
        .map_err(storage_err_to_api)?;
    let (source_key, dest_key) = match direction {
        TransferDirection::Open => ("unopened", "opened"),
        TransferDirection::Close => ("opened", "unopened"),
    };
    let source_deleted = source_after.is_none();
    Ok(Json(json!({
        source_key: source_after,
        dest_key: dest_after,
        "source_deleted": source_deleted,
    })))
}

fn perform_transfer(
    inv: &mut Inventory,
    appliance_id: &str,
    item_id: &str,
    direction: TransferDirection,
    mode: TransferMode,
) -> Result<(Option<InventoryItem>, InventoryItem), StorageError> {
    let source_idx = inv
        .items
        .iter()
        .position(|i| i.id == item_id && i.appliance_id == appliance_id)
        .ok_or_else(|| StorageError::Corrupt(format!("item {item_id} not found")))?;
    let source_snapshot = inv.items[source_idx].clone();

    let (pack_count, pack_label, pack_status) = match &source_snapshot.tracking {
        ItemTracking::Pack {
            pack_count,
            pack_label,
            pack_status,
        } => (*pack_count, pack_label.clone(), *pack_status),
        ItemTracking::Individual { .. } => {
            return Err(StorageError::Corrupt("item is not pack-based".into()));
        }
    };
    if pack_status != direction.source_status() {
        return Err(StorageError::Corrupt(match direction {
            TransferDirection::Open => "only unopened variants can be opened".into(),
            TransferDirection::Close => "only opened variants can be marked unopened".into(),
        }));
    }
    if pack_count == 0 {
        return Err(StorageError::Corrupt(match direction {
            TransferDirection::Open => "no unopened packs available to open".into(),
            TransferDirection::Close => "no opened packs available to mark unopened".into(),
        }));
    }

    let transfer = match mode {
        TransferMode::One => 1,
        TransferMode::All => pack_count,
    };

    // Auto-group: if the source item has no group, pair opened+unopened
    // variants by auto-creating a group named after the item so they visually
    // live together. Only applies when transferring Open → Unopened could
    // confuse users, so we only do this on the Open direction (which is where
    // the matching dest variant is auto-created).
    let effective_group_id =
        if source_snapshot.group_id.is_none() && matches!(direction, TransferDirection::Open) {
            let group_name = source_snapshot.name.trim().to_string();
            let existing = inv.groups.iter().find(|g| {
                g.appliance_id == source_snapshot.appliance_id
                    && g.name.eq_ignore_ascii_case(&group_name)
            });
            let gid = match existing {
                Some(g) => g.id.clone(),
                None => {
                    let g = Group {
                        id: new_id(),
                        name: group_name,
                        appliance_id: source_snapshot.appliance_id.clone(),
                        description: None,
                        created_at: now(),
                        updated_at: now(),
                    };
                    let id = g.id.clone();
                    inv.groups.push(g);
                    id
                }
            };
            inv.items[source_idx].group_id = Some(gid.clone());
            inv.items[source_idx].updated_at = now();
            Some(gid)
        } else {
            source_snapshot.group_id.clone()
        };

    let dest_status = direction.dest_status();
    let dest_idx = inv.items.iter().position(|i| {
        i.appliance_id == source_snapshot.appliance_id
            && i.group_id == effective_group_id
            && match &i.tracking {
                ItemTracking::Pack {
                    pack_label: lbl,
                    pack_status,
                    ..
                } => lbl == &pack_label && *pack_status == dest_status,
                _ => false,
            }
    });

    let dest_idx = match dest_idx {
        Some(idx) => idx,
        None => {
            let new_item = InventoryItem {
                id: new_id(),
                name: source_snapshot.name.clone(),
                appliance_id: source_snapshot.appliance_id.clone(),
                group_id: effective_group_id.clone(),
                category: source_snapshot.category.clone(),
                tracking: ItemTracking::Pack {
                    pack_count: 0,
                    pack_label: pack_label.clone(),
                    pack_status: dest_status,
                },
                expiration_date: source_snapshot.expiration_date.clone(),
                notes: source_snapshot.notes.clone(),
                has_photo: false,
                created_at: now(),
                updated_at: now(),
            };
            inv.items.push(new_item);
            inv.items.len() - 1
        }
    };

    if let ItemTracking::Pack {
        pack_count: ref mut dst,
        ..
    } = inv.items[dest_idx].tracking
    {
        let next = (*dst as u64)
            .checked_add(transfer as u64)
            .ok_or_else(|| StorageError::Corrupt("pack count overflow".into()))?;
        if next > u32::MAX as u64 {
            return Err(StorageError::Corrupt("pack count overflow".into()));
        }
        *dst = next as u32;
    }
    inv.items[dest_idx].updated_at = now();

    // The source index may have shifted if the dest was newly pushed before
    // the source — which can't happen (dest is pushed after source lookup),
    // but a group auto-create keeps source stable so we're fine.
    if let ItemTracking::Pack {
        pack_count: ref mut src,
        ..
    } = inv.items[source_idx].tracking
    {
        *src -= transfer;
    }
    inv.items[source_idx].updated_at = now();

    // Delete the source variant if it hit zero. Fetch id first so we can
    // return a snapshot after removal.
    let source_zero = matches!(
        &inv.items[source_idx].tracking,
        ItemTracking::Pack { pack_count: 0, .. }
    );
    let dest_item = inv.items[dest_idx].clone();
    let source_after = if source_zero {
        let removed = inv.items.remove(source_idx);
        // best-effort photo cleanup is caller's responsibility (filesystem/D1)
        drop(removed);
        None
    } else {
        Some(inv.items[source_idx].clone())
    };
    Ok((source_after, dest_item))
}

// ---------- groups ----------

#[derive(Deserialize)]
struct CreateGroupBody {
    name: String,
    appliance_id: String,
    description: Option<String>,
}

async fn list_groups(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    let session = require_auth(&state.accounts, &headers)?;
    let inv = state.inventory(&session.account_id)?;
    let groups = inv.with_read(|i| i.groups.clone());
    Ok(Json(json!({ "groups": groups })))
}

async fn create_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateGroupBody>,
) -> ApiResult<(StatusCode, Json<Group>)> {
    let session = require_auth(&state.accounts, &headers)?;
    require_csrf(&session, &headers)?;
    if !is_valid_name(&body.name) {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    let group = Group {
        id: new_id(),
        name: body.name.trim().to_string(),
        appliance_id: body.appliance_id.clone(),
        description: body.description.and_then(blank_to_none),
        created_at: now(),
        updated_at: now(),
    };
    let inv = state.inventory(&session.account_id)?;
    let created = inv
        .mutate(|i| {
            if !i.appliances.iter().any(|a| a.id == body.appliance_id) {
                return Err(StorageError::Corrupt(format!(
                    "appliance {} not found",
                    body.appliance_id
                )));
            }
            i.groups.push(group.clone());
            Ok(group.clone())
        })
        .map_err(storage_err_to_api)?;
    Ok((StatusCode::CREATED, Json(created)))
}

#[derive(Deserialize)]
struct UpdateGroupBody {
    name: Option<String>,
    description: Option<Option<String>>,
    appliance_id: Option<String>,
}

async fn update_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateGroupBody>,
) -> ApiResult<Json<Group>> {
    let session = require_auth(&state.accounts, &headers)?;
    require_csrf(&session, &headers)?;
    let inv = state.inventory(&session.account_id)?;
    let updated = inv.mutate(|i| {
        if let Some(ref new_appliance) = body.appliance_id {
            if !i.appliances.iter().any(|a| &a.id == new_appliance) {
                return Err(StorageError::Corrupt(format!(
                    "appliance {new_appliance} not found"
                )));
            }
        }
        let group = i
            .groups
            .iter_mut()
            .find(|g| g.id == id)
            .ok_or_else(|| StorageError::Corrupt(format!("group {id} not found")))?;
        if let Some(name) = body.name {
            if !is_valid_name(&name) {
                return Err(StorageError::Corrupt("name must not be empty".into()));
            }
            group.name = name.trim().to_string();
        }
        if let Some(description) = body.description {
            group.description = description.and_then(blank_to_none);
        }
        if let Some(new_appliance) = body.appliance_id {
            group.appliance_id = new_appliance;
        }
        group.updated_at = now();
        Ok(group.clone())
    });
    updated.map(Json).map_err(storage_err_to_api)
}

#[derive(Deserialize, Default)]
struct DeleteGroupParams {
    #[serde(default)]
    strategy: Option<String>,
    #[serde(default)]
    move_to_group: Option<String>,
}

async fn delete_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(params): Query<DeleteGroupParams>,
) -> ApiResult<StatusCode> {
    let session = require_auth(&state.accounts, &headers)?;
    require_csrf(&session, &headers)?;
    let inv = state.inventory(&session.account_id)?;
    let has_children =
        inv.with_read(|i| i.items.iter().any(|x| x.group_id.as_deref() == Some(&id)));
    let strategy = params.strategy.as_deref().unwrap_or("");
    if has_children && strategy != "cascade" && strategy != "detach" && strategy != "move" {
        return Err(ApiError::Conflict(
            "group has children; specify strategy=detach, strategy=move, or strategy=cascade"
                .into(),
        ));
    }
    inv.mutate(|i| {
        if !i.groups.iter().any(|g| g.id == id) {
            return Err(StorageError::Corrupt(format!("group {id} not found")));
        }
        match strategy {
            "cascade" => {
                i.items.retain(|x| x.group_id.as_deref() != Some(&id));
            }
            "detach" => {
                for item in i.items.iter_mut() {
                    if item.group_id.as_deref() == Some(&id) {
                        item.group_id = None;
                        item.updated_at = now();
                    }
                }
            }
            "move" => {
                let target = params
                    .move_to_group
                    .clone()
                    .ok_or_else(|| StorageError::Corrupt("move_to_group is required".into()))?;
                if !i.groups.iter().any(|g| g.id == target) {
                    return Err(StorageError::Corrupt(format!(
                        "target group {target} not found"
                    )));
                }
                for item in i.items.iter_mut() {
                    if item.group_id.as_deref() == Some(&id) {
                        item.group_id = Some(target.clone());
                        item.updated_at = now();
                    }
                }
            }
            _ => {}
        }
        i.groups.retain(|g| g.id != id);
        Ok(())
    })
    .map_err(storage_err_to_api)?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- global items ----------

async fn list_items(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    let session = require_auth(&state.accounts, &headers)?;
    let inv = state.inventory(&session.account_id)?;
    let (items, groups, appliances) =
        inv.with_read(|i| (i.items.clone(), i.groups.clone(), i.appliances.clone()));
    Ok(Json(json!({
        "items": items,
        "groups": groups,
        "appliances": appliances,
    })))
}

// ---------- photos ----------

/// Max accepted base64 payload — ~1.5 MB of binary. Clients should resize
/// to well under this before uploading.
const MAX_PHOTO_BYTES_B64: usize = 2 * 1024 * 1024;

#[derive(Deserialize)]
struct PhotoUploadBody {
    /// Base64-encoded JPEG. Must decode to valid JPEG bytes.
    image_base64: String,
}

async fn upload_photo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((appliance_id, item_id)): Path<(String, String)>,
    Json(body): Json<PhotoUploadBody>,
) -> ApiResult<Json<InventoryItem>> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    let session = require_auth(&state.accounts, &headers)?;
    require_csrf(&session, &headers)?;

    if body.image_base64.len() > MAX_PHOTO_BYTES_B64 {
        return Err(ApiError::BadRequest(
            "photo too large (resize to under ~1.5 MB)".to_string(),
        ));
    }
    let bytes = STANDARD
        .decode(body.image_base64.as_bytes())
        .map_err(|_| ApiError::BadRequest("image_base64 is not valid base64".to_string()))?;
    if !looks_like_jpeg(&bytes) {
        return Err(ApiError::BadRequest(
            "only JPEG uploads are accepted (resize client-side to JPEG)".to_string(),
        ));
    }

    let path = state
        .photo_path(&session.account_id, &item_id)
        .ok_or_else(|| ApiError::BadRequest("invalid item id".to_string()))?;

    // Confirm the item exists before we write a file.
    let inv = state.inventory(&session.account_id)?;
    inv.with_read(|i| {
        if !i
            .items
            .iter()
            .any(|x| x.id == item_id && x.appliance_id == appliance_id)
        {
            return Err(ApiError::NotFound(format!("item {item_id} not found")));
        }
        Ok(())
    })?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ApiError::Internal(format!("photo dir create: {e}")))?;
    }
    // Atomic write: temp file + rename.
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "photo.jpg".to_string());
    let tmp = path
        .parent()
        .map(|p| p.join(format!(".{file_name}.tmp")))
        .ok_or_else(|| ApiError::Internal("photo path has no parent".to_string()))?;
    std::fs::write(&tmp, &bytes).map_err(|e| ApiError::Internal(format!("photo write: {e}")))?;
    std::fs::rename(&tmp, &path).map_err(|e| ApiError::Internal(format!("photo rename: {e}")))?;

    // Flip has_photo = true on the item.
    let updated = inv
        .mutate(|i| {
            let item = i
                .items
                .iter_mut()
                .find(|x| x.id == item_id && x.appliance_id == appliance_id)
                .ok_or_else(|| StorageError::Corrupt(format!("item {item_id} not found")))?;
            item.has_photo = true;
            item.updated_at = now();
            Ok(item.clone())
        })
        .map_err(storage_err_to_api)?;
    Ok(Json(updated))
}

async fn get_photo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((appliance_id, item_id)): Path<(String, String)>,
) -> ApiResult<Response> {
    let session = require_auth(&state.accounts, &headers)?;
    // Belt-and-braces: don't serve a photo for an item that isn't yours.
    let inv = state.inventory(&session.account_id)?;
    inv.with_read(|i| {
        if !i
            .items
            .iter()
            .any(|x| x.id == item_id && x.appliance_id == appliance_id)
        {
            return Err(ApiError::NotFound(format!("item {item_id} not found")));
        }
        Ok(())
    })?;
    let path = state
        .photo_path(&session.account_id, &item_id)
        .ok_or_else(|| ApiError::BadRequest("invalid item id".to_string()))?;
    let bytes =
        std::fs::read(&path).map_err(|_| ApiError::NotFound("photo not found".to_string()))?;
    let mut resp = Response::new(axum::body::Body::from(bytes));
    resp.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
    // Private per-account content: never cache in shared proxies or the SW.
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok(resp)
}

async fn delete_photo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((appliance_id, item_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let session = require_auth(&state.accounts, &headers)?;
    require_csrf(&session, &headers)?;
    let inv = state.inventory(&session.account_id)?;
    inv.mutate(|i| {
        let item = i
            .items
            .iter_mut()
            .find(|x| x.id == item_id && x.appliance_id == appliance_id)
            .ok_or_else(|| StorageError::Corrupt(format!("item {item_id} not found")))?;
        item.has_photo = false;
        item.updated_at = now();
        Ok(())
    })
    .map_err(storage_err_to_api)?;
    if let Some(path) = state.photo_path(&session.account_id, &item_id) {
        let _ = std::fs::remove_file(path);
    }
    Ok(StatusCode::NO_CONTENT)
}

fn looks_like_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF
}

// ---------- utils ----------

fn blank_to_none(s: String) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s.trim().to_string())
    }
}

fn storage_err_to_api(e: StorageError) -> ApiError {
    match &e {
        StorageError::Corrupt(msg) => {
            if msg.contains("not found") {
                ApiError::NotFound(msg.clone())
            } else {
                ApiError::BadRequest(msg.clone())
            }
        }
        _ => ApiError::Internal(e.to_string()),
    }
}
