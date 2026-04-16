use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ChangedItem {
    pub id: i32,
    pub adult: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChangedPage {
    pub results: Vec<ChangedItem>,
    pub page: i32,
    pub total_pages: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PropertyChanges {
    pub changes: Vec<PropertyChange>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PropertyChange {
    pub key: String,
    pub items: Vec<ChangeItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChangeItem {
    pub id: String,
    pub action: String,
    pub time: String,
    pub iso_639_1: Option<String>,
    pub value: Option<serde_json::Value>,
    pub original_value: Option<serde_json::Value>,
}
