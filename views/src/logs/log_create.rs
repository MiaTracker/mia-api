use chrono::NaiveDate;
use sea_orm::ActiveValue::Set;
use sea_orm::NotSet;
use serde::Deserialize;
use entities::logs;
use crate::infrastructure::traits::IntoActiveModel;

#[derive(Deserialize)]
pub struct LogCreate {
    pub date: NaiveDate,
    pub rating: Option<f32>,
    pub completed: bool,
    pub comment: Option<String>
}

impl IntoActiveModel<logs::ActiveModel> for &LogCreate {
    fn into_active_model(self) -> logs::ActiveModel {
        logs::ActiveModel {
            id: NotSet,
            media_id: NotSet,
            date: Set(self.date),
            rating: Set(self.rating),
            completed: Set(self.completed),
            comment: Set(self.comment.clone()),
            user_id: NotSet,
        }
    }
}