use serde::Deserialize;

#[derive(Deserialize)]
pub struct TagDeleteParams {
    pub media_id: i32,
    pub tag_id: i32
}