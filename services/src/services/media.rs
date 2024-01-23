use std::env;
use sea_orm::{ColumnTrait, DbConn, EntityTrait, ModelTrait, TransactionTrait};
use sea_orm::prelude::Expr;
use sea_orm::sea_query::Query;
use sea_orm::QueryFilter;
use entities::{genres, media, media_genres, media_tags, sea_orm_active_enums, tags, titles};
use entities::prelude::{Genres, MediaGenres, MediaTags, Tags, Titles};
use views::media::{MediaIndex, MediaType};
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;
use crate::services;

pub async fn search(query: String, media_type: Option<MediaType>, user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let query = services::query::parse(query, media_type);
    let select = services::query::build_sql_query(query, user);
    let media = select.all(db).await?;
    let indexes = build_media_indexes(media, db).await?;
    Ok(indexes)
}

pub async fn delete(media_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into())).one(db).await?;

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

pub(crate) async fn build_media_indexes(media: Vec<media::Model>, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let mut indexes = Vec::with_capacity(media.len());
    for m in media {
        let title = m.find_related(Titles).filter(titles::Column::Primary.eq(true)).one(db).await?;
        let title = if let Some(title) = title {
            title.title
        } else { env::var("UNSET_MEDIA_TITLE").expect("UNSET_MEDIA_TITLE not set!") };

        let index = MediaIndex {
            id: m.id,
            r#type: views::media::MediaType::from(m.r#type),
            poster_path: m.poster_path,
            stars: m.stars,
            title,
        };
        indexes.push(index);
    }

    Ok(indexes)
}