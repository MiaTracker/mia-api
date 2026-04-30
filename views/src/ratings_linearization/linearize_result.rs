use serde::Serialize;
use super::user_summary::UserSummary;

#[derive(Serialize)]
pub struct LinearizeResult {
    pub dry_run: bool,
    pub users_processed: u32,
    pub users_skipped: u32,
    pub logs_updated: u32,
    pub media_recomputed: u32,
    pub per_user: Vec<UserSummary>,
}
