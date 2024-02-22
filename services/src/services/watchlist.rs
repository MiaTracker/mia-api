use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, IntoActiveModel, ModelTrait, NotSet, PaginatorTrait, QueryFilter, QueryOrder, Set};
use sea_orm::sea_query::Query;
use entities::{media, titles, watchlist};
use entities::prelude::{Media, Titles, Watchlist};
use views::media::MediaIndex;
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;
use crate::services;

pub async fn add(media_id: i32, user: &CurrentUser, db: &DbConn) -> Result<bool, SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::UserId.eq(user.id)).one(db).await?;
    if media.is_none() {
        return Err(SrvErr::NotFound);
    }

    let already_added = watchlist::Entity::find().filter(watchlist::Column::MediaId.eq(media_id)).count(db).await? != 0;
    if already_added {
        return Ok(false);
    }

    let am = watchlist::ActiveModel {
        media_id: Set(media_id),
        assessment: NotSet,
        date_added: Set(chrono::Utc::now().date_naive()),
    };
    am.insert(db).await?;

    let media = media.unwrap();
    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.into_active_model();
        media_am.bot_controllable = Set(bot_controllable);
        media_am.update(db).await?;
    }

    Ok(true)
}

pub async fn index(user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let media_w_titles = Media::find().right_join(Watchlist)
        .filter(media::Column::UserId.eq(user.id))
        .find_also_related(Titles)
        .filter(titles::Column::Primary.eq(true))
        .order_by_asc(titles::Column::Title)
        .all(db).await?;
    let indexes = services::media::build_media_indexes(media_w_titles);
    Ok(indexes)
}

pub async fn search(query: String, user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let query = services::query::parse(query, None);
    let select = services::query::build_sql_query(query, user)
        .filter(media::Column::Id.in_subquery(Query::select()
            .column(watchlist::Column::MediaId).from(Watchlist).to_owned()));
    let media_w_titles = select.all(db).await?;

    let indexes = services::media::build_media_indexes(media_w_titles);

    Ok(indexes)
}


pub async fn remove(media_id: i32, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let watch = watchlist::Entity::find().filter(watchlist::Column::MediaId.eq(media_id))
        .inner_join(Media).filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if watch.is_none() {
        return Err(SrvErr::NotFound);
    }
    watch.unwrap().delete(db).await?;

    Ok(())
}