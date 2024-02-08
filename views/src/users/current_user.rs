use uuid::Uuid;

#[derive(Clone)]
pub struct CurrentUser {
    pub id: i32,
    pub uuid: Uuid,
    pub email: String,
    pub username: String,
    pub admin: bool,
    pub though_bot: bool
}