use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, QueryFilter};
use entities::logs;
use views::infrastructure::traits::IntoActiveModel;
use views::logs::LogCreate;
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;

pub async fn create(media_id: i32, log: &LogCreate, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let mut model = log.into_active_model();
    model.media_id = Set(media_id);
    model.user_id = Set(user.id);
    model.insert(db).await?;
    Ok(())
}

pub async fn delete(media_id: i32, log_id: i32, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let res = logs::Entity::delete_by_id(log_id)
        .filter(logs::Column::MediaId.eq(media_id))
        .filter(logs::Column::UserId.eq(user.id))
        .exec(db)
        .await?;
    if res.rows_affected != 1 {
        Err(SrvErr::NotFound)
    } else {
        Ok(())
    }
}