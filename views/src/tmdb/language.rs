use sea_orm::ActiveValue::Set;
use serde::Deserialize;
use entities::languages;
use crate::infrastructure::traits::IntoActiveModel;

#[derive(Deserialize, Clone)]
pub struct Languages {
    pub english_name: String,
    pub iso_639_1: String,
    pub name: String
}

impl IntoActiveModel<languages::ActiveModel> for &Languages {
    fn into_active_model(self) -> languages::ActiveModel {
        languages::ActiveModel {
            iso6391: Set(self.iso_639_1.clone()),
            english_name: Set(self.english_name.clone()),
            name: Set(self.name.clone()),
        }
    }
}