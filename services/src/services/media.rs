use std::env;
use sea_orm::{ColumnTrait, DbConn, EntityTrait, ModelTrait, QueryOrder, TransactionTrait};
use sea_orm::prelude::Expr;
use sea_orm::sea_query::Query;
use sea_orm::QueryFilter;
use entities::{genres, media, media_genres, media_tags, sea_orm_active_enums, tags, titles};
use entities::prelude::{Genres, Media, MediaGenres, MediaTags, Tags, Titles};
use views::media::{MediaIndex, MediaType};
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;
use crate::services;

pub async fn index(user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let media_w_titles = Media::find().filter(media::Column::UserId.eq(user.id)).find_also_related(Titles)
        .filter(titles::Column::Primary.eq(true)).order_by_asc(titles::Column::Title).all(db).await?;
    let indexes = build_media_indexes(media_w_titles).await?;
    Ok(indexes)
}

pub async fn search(query: String, media_type: Option<MediaType>, user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let query = services::query::parse(query, media_type);
    let select = services::query::build_sql_query(query, user);
    let media_w_titles = select.all(db).await?;
    let indexes = build_media_indexes(media_w_titles).await?;
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

pub(crate) async fn build_media_indexes(media_w_titles: Vec<(media::Model, Option<titles::Model>)>) -> Result<Vec<MediaIndex>, SrvErr> {
    let mut indexes = Vec::with_capacity(media_w_titles.len());
    for m in media_w_titles {
        let title = if let Some(title) = m.1 {
            title.title
        } else { env::var("UNSET_MEDIA_TITLE").expect("UNSET_MEDIA_TITLE not set!") };

        let index = MediaIndex {
            id: m.0.id,
            r#type: views::media::MediaType::from(m.0.r#type),
            poster_path: m.0.poster_path,
            stars: m.0.stars,
            title,
        };
        indexes.push(index);
    }

    indexes.sort_by(|x, y| {
        let t1l = x.title.to_lowercase();
        let t1 = t1l.trim_start_matches("the ");
        let t2l = y.title.to_lowercase();
        let t2 = t2l.trim_start_matches("the ");
        t1.cmp(t2)
    });

    Ok(indexes)
}