use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::storage::{JsonStore, StorageError};

pub const SESSION_COOKIE: &str = "chillventory_session";
pub const CSRF_HEADER: &str = "x-chillventory-csrf";
pub const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub pin_hash: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AccountsFile {
    #[serde(default)]
    pub accounts: Vec<Account>,
}

pub type AccountsStore = JsonStore<AccountsFile>;

#[derive(Debug, Clone)]
struct Session {
    account_id: String,
    csrf: String,
    last_seen: Instant,
}

#[derive(Debug, Clone)]
struct AttemptRecord {
    failures: u32,
    locked_until: Option<Instant>,
}

pub struct AccountsState {
    store: AccountsStore,
    sessions: Mutex<HashMap<String, Session>>,
    attempts: Mutex<HashMap<IpAddr, AttemptRecord>>,
    session_ttl: Duration,
}

#[derive(Debug)]
pub enum AuthError {
    Storage(StorageError),
    NoAccounts,
    NameTaken,
    NameTooShort,
    NameTooLong,
    InvalidCredentials,
    RateLimited(Duration),
    Hash(String),
    PinTooWeak,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::Storage(e) => write!(f, "storage error: {e}"),
            AuthError::NoAccounts => write!(f, "no accounts configured"),
            AuthError::NameTaken => write!(f, "that name is already in use"),
            AuthError::NameTooShort => write!(f, "name must be at least 1 character"),
            AuthError::NameTooLong => write!(f, "name is too long"),
            AuthError::InvalidCredentials => write!(f, "incorrect name or PIN"),
            AuthError::RateLimited(d) => {
                write!(f, "too many attempts, retry after {}s", d.as_secs())
            }
            AuthError::Hash(e) => write!(f, "hashing error: {e}"),
            AuthError::PinTooWeak => write!(f, "pin must be 4-12 digits"),
        }
    }
}

impl std::error::Error for AuthError {}

impl From<StorageError> for AuthError {
    fn from(value: StorageError) -> Self {
        AuthError::Storage(value)
    }
}

#[derive(Debug)]
pub struct SessionInfo {
    pub id: String,
    pub csrf: String,
    pub account_id: String,
}

impl AccountsState {
    pub fn load(path: PathBuf) -> Result<Self, StorageError> {
        let store = AccountsStore::load_or_init(path)?;
        Ok(Self {
            store,
            sessions: Mutex::new(HashMap::new()),
            attempts: Mutex::new(HashMap::new()),
            session_ttl: DEFAULT_SESSION_TTL,
        })
    }

    pub fn with_session_ttl(mut self, ttl: Duration) -> Self {
        self.session_ttl = ttl;
        self
    }

    pub fn account_count(&self) -> usize {
        self.store.with_read(|f| f.accounts.len())
    }

    pub fn has_accounts(&self) -> bool {
        self.account_count() > 0
    }

    pub fn account_names(&self) -> Vec<String> {
        self.store
            .with_read(|f| f.accounts.iter().map(|a| a.name.clone()).collect())
    }

    pub fn account(&self, id: &str) -> Option<Account> {
        self.store
            .with_read(|f| f.accounts.iter().find(|a| a.id == id).cloned())
    }

    pub fn signup(&self, name: &str, pin: &str) -> Result<SessionInfo, AuthError> {
        let name = validate_name(name)?;
        validate_pin(pin)?;
        let hash = hash_pin(pin)?;
        let account_id = uuid::Uuid::new_v4().to_string();
        let created_id = account_id.clone();
        self.store
            .mutate(|f| {
                if f.accounts
                    .iter()
                    .any(|a| a.name.eq_ignore_ascii_case(&name))
                {
                    return Err(StorageError::Corrupt("name taken".into()));
                }
                f.accounts.push(Account {
                    id: created_id.clone(),
                    name: name.clone(),
                    pin_hash: hash,
                    created_at: OffsetDateTime::now_utc(),
                    updated_at: OffsetDateTime::now_utc(),
                });
                Ok(())
            })
            .map_err(|e| match e {
                StorageError::Corrupt(ref m) if m == "name taken" => AuthError::NameTaken,
                other => AuthError::Storage(other),
            })?;
        Ok(self.create_session(&account_id))
    }

    pub fn login(
        &self,
        name: &str,
        pin: &str,
        peer: Option<IpAddr>,
    ) -> Result<SessionInfo, AuthError> {
        if let Some(ip) = peer {
            self.check_rate_limit(ip)?;
        }
        let stored = self.store.with_read(|f| {
            f.accounts
                .iter()
                .find(|a| a.name.eq_ignore_ascii_case(name))
                .cloned()
        });
        // Always run a verify, even on unknown name, to equalize timing.
        let ok = match &stored {
            Some(acc) => verify_pin_hash(pin, &acc.pin_hash)?,
            None => {
                let _ = verify_pin_hash(pin, DUMMY_HASH);
                false
            }
        };
        if !ok {
            if let Some(ip) = peer {
                self.record_failure(ip);
            }
            return Err(AuthError::InvalidCredentials);
        }
        if let Some(ip) = peer {
            self.record_success(ip);
        }
        let account_id = stored.unwrap().id;
        Ok(self.create_session(&account_id))
    }

    pub fn change_pin(
        &self,
        account_id: &str,
        current: &str,
        new_pin: &str,
    ) -> Result<(), AuthError> {
        validate_pin(new_pin)?;
        let stored = self
            .store
            .with_read(|f| f.accounts.iter().find(|a| a.id == account_id).cloned())
            .ok_or(AuthError::InvalidCredentials)?;
        if !verify_pin_hash(current, &stored.pin_hash)? {
            return Err(AuthError::InvalidCredentials);
        }
        let new_hash = hash_pin(new_pin)?;
        let acct_id = account_id.to_string();
        self.store.mutate(|f| {
            if let Some(a) = f.accounts.iter_mut().find(|a| a.id == acct_id) {
                a.pin_hash = new_hash;
                a.updated_at = OffsetDateTime::now_utc();
            }
            Ok(())
        })?;
        // Revoke all sessions for this account.
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|_, s| s.account_id != account_id);
        Ok(())
    }

    pub fn session(&self, id: &str) -> Option<SessionInfo> {
        let mut guard = self.sessions.lock().unwrap();
        let entry = guard.get_mut(id)?;
        if entry.last_seen.elapsed() > self.session_ttl {
            guard.remove(id);
            return None;
        }
        entry.last_seen = Instant::now();
        Some(SessionInfo {
            id: id.to_string(),
            csrf: entry.csrf.clone(),
            account_id: entry.account_id.clone(),
        })
    }

    pub fn revoke(&self, id: &str) {
        self.sessions.lock().unwrap().remove(id);
    }

    fn create_session(&self, account_id: &str) -> SessionInfo {
        let id = random_token(32);
        let csrf = random_token(32);
        self.sessions.lock().unwrap().insert(
            id.clone(),
            Session {
                account_id: account_id.to_string(),
                csrf: csrf.clone(),
                last_seen: Instant::now(),
            },
        );
        SessionInfo {
            id,
            csrf,
            account_id: account_id.to_string(),
        }
    }

    fn check_rate_limit(&self, ip: IpAddr) -> Result<(), AuthError> {
        let guard = self.attempts.lock().unwrap();
        if let Some(record) = guard.get(&ip) {
            if let Some(until) = record.locked_until {
                let now = Instant::now();
                if until > now {
                    return Err(AuthError::RateLimited(until - now));
                }
            }
        }
        Ok(())
    }

    fn record_failure(&self, ip: IpAddr) {
        let mut guard = self.attempts.lock().unwrap();
        let entry = guard.entry(ip).or_insert(AttemptRecord {
            failures: 0,
            locked_until: None,
        });
        entry.failures = entry.failures.saturating_add(1);
        let penalty = match entry.failures {
            0..=4 => Duration::from_secs(0),
            5..=9 => Duration::from_secs(5),
            10..=14 => Duration::from_secs(30),
            15..=19 => Duration::from_secs(120),
            _ => Duration::from_secs(300),
        };
        if !penalty.is_zero() {
            entry.locked_until = Some(Instant::now() + penalty);
        }
    }

    fn record_success(&self, ip: IpAddr) {
        self.attempts.lock().unwrap().remove(&ip);
    }
}

/// A pre-computed argon2 hash of a fixed dummy PIN. Used so that failed logins
/// against unknown account names still perform a verify, equalizing timing and
/// preventing name enumeration via response time.
const DUMMY_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$ZHVtbXlzYWx0ZHVtbXk$R6k1T3h04vz9Ck9Cz1zXwQ";

fn validate_name(name: &str) -> Result<String, AuthError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AuthError::NameTooShort);
    }
    if trimmed.chars().count() > 60 {
        return Err(AuthError::NameTooLong);
    }
    Ok(trimmed.to_string())
}

fn validate_pin(pin: &str) -> Result<(), AuthError> {
    let len = pin.chars().count();
    if !(4..=12).contains(&len) || !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(AuthError::PinTooWeak);
    }
    Ok(())
}

fn hash_pin(pin: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    let hasher = Argon2::default();
    hasher
        .hash_password(pin.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AuthError::Hash(e.to_string()))
}

fn verify_pin_hash(pin: &str, stored_hash: &str) -> Result<bool, AuthError> {
    let parsed = PasswordHash::new(stored_hash).map_err(|e| AuthError::Hash(e.to_string()))?;
    Ok(Argon2::default()
        .verify_password(pin.as_bytes(), &parsed)
        .is_ok())
}

fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buf);
    hex::encode(buf)
}

pub fn constant_time_eq(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq;
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    a_bytes.ct_eq(b_bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_state() -> (AccountsState, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let state = AccountsState::load(dir.path().join("accounts.json")).unwrap();
        (state, dir)
    }

    #[test]
    fn signup_then_login_round_trip() {
        let (state, _d) = make_state();
        let sess = state.signup("Pranee", "4242").unwrap();
        assert!(!sess.account_id.is_empty());
        let login = state.login("Pranee", "4242", None).unwrap();
        assert_eq!(login.account_id, sess.account_id);
    }

    #[test]
    fn login_case_insensitive_name() {
        let (state, _d) = make_state();
        state.signup("Pranee", "4242").unwrap();
        state.login("pranee", "4242", None).unwrap();
        state.login("PRANEE", "4242", None).unwrap();
    }

    #[test]
    fn duplicate_name_rejected() {
        let (state, _d) = make_state();
        state.signup("Pranee", "4242").unwrap();
        let err = state.signup("pranee", "1111").unwrap_err();
        assert!(matches!(err, AuthError::NameTaken));
    }

    #[test]
    fn wrong_name_and_wrong_pin_look_identical() {
        let (state, _d) = make_state();
        state.signup("Pranee", "4242").unwrap();
        let a = state.login("Pranee", "0000", None).unwrap_err();
        let b = state.login("Nobody", "4242", None).unwrap_err();
        assert!(matches!(a, AuthError::InvalidCredentials));
        assert!(matches!(b, AuthError::InvalidCredentials));
    }

    #[test]
    fn change_pin_verifies_current() {
        let (state, _d) = make_state();
        let sess = state.signup("Pranee", "4242").unwrap();
        assert!(state.change_pin(&sess.account_id, "0000", "1111").is_err());
        state.change_pin(&sess.account_id, "4242", "1111").unwrap();
        state.login("Pranee", "1111", None).unwrap();
    }

    #[test]
    fn pin_hash_does_not_contain_pin() {
        let hash = hash_pin("4242").unwrap();
        assert!(!hash.contains("4242"));
    }
}
