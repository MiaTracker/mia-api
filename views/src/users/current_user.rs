use uuid::Uuid;
use entities::users;

#[derive(Clone)]
pub struct CurrentUser {
    pub id: i32,
    pub uuid: Uuid,
    pub email: String,
    pub username: String,
    pub admin: bool
}

impl From<users::Model> for CurrentUser {
    fn from(value: users::Model) -> Self {
        Self {
            id: value.id,
            uuid: value.uuid,
            email: value.email,
            username: value.username,
            admin: value.admin
        }
    }
}