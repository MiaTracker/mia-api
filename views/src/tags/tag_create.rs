use sea_orm::ActiveValue::Set;
use sea_orm::NotSet;
use serde::Deserialize;
use entities::tags;
use crate::infrastructure::traits::IntoActiveModel;

#[derive(Deserialize)]
pub struct TagCreate {
    pub name: String
}

impl IntoActiveModel<tags::ActiveModel> for &TagCreate {
    fn into_active_model(self) -> tags::ActiveModel {
        tags::ActiveModel {
            id: NotSet,
            name: Set(self.name.clone()),
        }
    }
}