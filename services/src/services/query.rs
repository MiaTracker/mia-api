use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect, Select};
use sea_orm::prelude::Expr;
use sea_orm::sea_query::extension::postgres::PgExpr;
use entities::prelude::{Media, Titles};
use crate::views::query::Query;
use entities::{media, sea_orm_active_enums, titles};
use views::media::MediaType;
use views::users::CurrentUser;

pub fn parse(query: String, media_type: Option<MediaType>) -> Query {
    Query {
        title: query,
        media_type,
    }
}

pub fn build_sql_query(query: Query, user: &CurrentUser) -> Select<media::Entity> {
    let mut select = Media::find()
        .filter(media::Column::UserId.eq(user.id))
        .inner_join(Titles)
        .filter(Expr::col(titles::Column::Title).ilike(format!("{}%", query.title)))
        .distinct();

    if let Some(media_type) = query.media_type {
        select = select.filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()));
    }

    select
}