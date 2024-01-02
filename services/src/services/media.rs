use sea_orm::{ColumnTrait, DbConn, EntityTrait, ModelTrait, TransactionTrait};
use sea_orm::prelude::Expr;
use sea_orm::sea_query::Query;
use sea_orm::QueryFilter;
use entities::{genres, media, media_genres, media_tags, tags};
use entities::prelude::{Genres, MediaGenres, MediaTags, Tags};
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;

pub async fn delete(media_id: i32, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let tran = db.begin().await?;
    media.delete(db).await?;

    Tags::delete_many().filter(tags::Column::Id.in_subquery(
        Query::select().column(tags::Column::Id).from(Tags)
            .left_join(MediaTags, Expr::col((Tags, tags::Column::Id)).equals((MediaTags, media_tags::Column::TagId)))
            .cond_where(media_tags::Column::TagId.is_null())
            .to_owned())
    ).exec(db).await?;

    Genres::delete_many().filter(genres::Column::Id.in_subquery(
        Query::select().column(genres::Column::Id).from(Genres)
            .left_join(MediaGenres, Expr::col((Genres, genres::Column::Id)).equals((MediaGenres, media_genres::Column::GenreId)))
            .cond_where(media_genres::Column::GenreId.is_null())
            .and_where(genres::Column::TmdbId.is_null())
            .to_owned())
    ).exec(db).await?;

    tran.commit().await?;

    Ok(())
}