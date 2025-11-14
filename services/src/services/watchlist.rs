use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, IntoActiveModel, ModelTrait, NotSet, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set};
use sea_orm::sea_query::Query;
use entities::{functions, image_sizes, media, titles, watchlist};
use entities::prelude::{ImageSizes, Media, Titles, Watchlist};
use entities::traits::linked::MediaPosters;
use views::media::{MediaIndex, PageReq, SearchQuery, SearchResults};
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

pub async fn index(page_req: PageReq, user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let media = Media::find().right_join(Watchlist)
        .filter(media::Column::UserId.eq(user.id))
        .find_also_linked(MediaPosters)
        .find_also_related(Titles)
        .filter(titles::Column::Primary.eq(true))
        .order_by_asc(functions::default_media_sort())
        .offset(page_req.offset).limit(page_req.limit).all(db).await?;

    let poster_ids: Vec<i32> = media.iter().filter_map(|p| p.0.poster_image_id).collect();
    let poster_sizes = ImageSizes::find()
        .filter(image_sizes::Column::ImageId.is_in(poster_ids)).all(db).await?;

    let indexes = services::media::build_media_indexes(media, poster_sizes);
    Ok(indexes)
}

pub async fn search(query: SearchQuery, page_req: PageReq, user: &CurrentUser, db: &DbConn) -> Result<SearchResults, SrvErr> {
    let res = transpiler::transpile(query, page_req, user, None);
    if res.is_err() {
        return Ok(SearchResults {
            indexes: vec![],
            external: vec![],
            query_valid: false,
        });
    }
    let res = res.ok().unwrap();

    let media = res.query.filter(media::Column::Id.in_subquery(Query::select()
            .column(watchlist::Column::MediaId).from(Watchlist).to_owned()))
        .all(db).await?;

    let poster_ids: Vec<i32> = media.iter().filter_map(|p| p.0.poster_image_id).collect();
    let poster_sizes = ImageSizes::find()
        .filter(image_sizes::Column::ImageId.is_in(poster_ids)).all(db).await?;


    let indexes = services::media::build_media_indexes(media, poster_sizes);

    Ok(SearchResults {
        indexes,
        external: vec![],
        query_valid: true,
    })
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