use sea_orm::DbConn;
use views::logs::LogCreate;
use views::logset::LogsetCreate;
use views::media::MediaType;
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;

pub async fn create(logset: &LogsetCreate, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    if logset.media_id.is_some() && logset.external_id.is_some() {
        return Err(SrvErr::BadRequest("Client sent both media_id and external_id. Only one of them must be set.".to_string()))
    }
    if logset.media_id.is_none() && logset.external_id.is_none() {
        return Err(SrvErr::BadRequest("Client sent neither media_id nor external_id. One of them must be set.".to_string()))
    }

    if logset.source.is_some() && logset.new_source.is_some() {
        return Err(SrvErr::BadRequest("Client sent both source and new_source. Only one of them must be set.".to_string()))
    }
    if logset.source.is_none() && logset.new_source.is_none() {
        return Err(SrvErr::BadRequest("Client sent neither source nor new_source. One of them must be set.".to_string()))
    }

    let media_id;
    if let Some(external) = logset.external_id {
        if logset.media_type == MediaType::Movie {
            let (_, id) = crate::services::movies::create(external, user, db).await?;
            media_id = id
        }
        else {
            let (_, id) = crate::services::series::create(external, user, db).await?;
            media_id = id
        }
    } else { media_id = logset.media_id.unwrap() }

    let source_name;
    if let Some(source) = &logset.new_source {
        crate::services::sources::create(media_id, &source, logset.media_type, user, db).await?;
        source_name = source.name.clone();
    } else { source_name = logset.source.clone().unwrap() }

    let log = LogCreate {
        date: logset.date,
        stars: logset.stars,
        completed: logset.completed,
        comment: logset.comment.clone(),
        source: source_name,
    };

    crate::services::logs::create(media_id, &log, logset.media_type, user, db).await?;

    if logset.remove_from_watchlist == Some(true) {
        crate::services::watchlist::remove(media_id, user, db).await?;
    }

    Ok(())
}