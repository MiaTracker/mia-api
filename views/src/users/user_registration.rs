use sea_orm::ActiveValue::Set;
use sea_orm::NotSet;
use serde::Deserialize;
use entities::users;
use entities::users::ActiveModel;
use crate::infrastructure::traits::IntoActiveModel;

#[derive(Deserialize)]
pub struct UserRegistration {
    pub email: String,
    pub username: String,
    pub password: String,
    pub password_repeat: String
}

impl IntoActiveModel<users::ActiveModel> for &UserRegistration {
    fn into_active_model(self) -> ActiveModel {
        users::ActiveModel {
            id: NotSet,
            uuid: Set(uuid::Uuid::new_v4()),
            email: Set(self.email.clone()),
            username: Set(self.username.clone()),
            password_hash: NotSet,
            admin: Set(false),
        }
    }
}