use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ApplianceKind {
    Fridge,
    Freezer,
    Shelf,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Appliance {
    pub id: String,
    pub name: String,
    pub kind: ApplianceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub appliance_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PackStatus {
    Opened,
    Unopened,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tracking", rename_all = "snake_case")]
pub enum ItemTracking {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryItem {
    pub id: String,
    pub name: String,
    pub appliance_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(flatten)]
    pub tracking: ItemTracking,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiration_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default)]
    pub has_photo: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Inventory {
    #[serde(default)]
    pub appliances: Vec<Appliance>,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub items: Vec<InventoryItem>,
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

pub fn is_finite_nonneg(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

pub fn is_valid_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty() && trimmed.chars().count() <= 200
}

pub fn is_valid_iso_date(s: &str) -> bool {
    use time::format_description::well_known::Iso8601;
    time::Date::parse(s, &Iso8601::DEFAULT).is_ok()
}
