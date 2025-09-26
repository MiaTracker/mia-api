use std::env;

use futures::TryFutureExt;
use http::StatusCode;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, IntoActiveModel, ModelTrait, PaginatorTrait, QueryOrder, QuerySelect, TransactionTrait};
use sea_orm::ActiveValue::Set;
use sea_orm::prelude::Expr;
use sea_orm::QueryFilter;
use sea_orm::sea_query::Query;

use entities::{functions, genres, languages, media, media_genres, media_tags, sea_orm_active_enums, tags, titles, watchlist};
use entities::prelude::{Genres, Languages, Media, MediaGenres, MediaTags, Sources, Tags, Titles};
use integrations::tmdb;
use integrations::tmdb::views::MultiResult;
use views::images::{BackdropUpdate, Image, Images, ImagesUpdate, PosterUpdate};
use views::media::{MediaIndex, MediaSourceCreate, MediaType, PageReq, SearchQuery, SearchResults};
use views::users::CurrentUser;

use crate::{movies, series, sources};
use crate::infrastructure::SrvErr;
use crate::infrastructure::traits::{IntoImage, IntoView, SortCompare};
use crate::sources::delete_from_media;

pub async fn create_w_source(view: MediaSourceCreate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(bool, i32), SrvErr> {
    let (created, id) = match media_type {
        MediaType::Movie => { movies::create(view.tmdb_id, user, db).await? }
        MediaType::Series => { series::create(view.tmdb_id, user, db).await? }
    };

    sources::create(id, &view.source, media_type, &user, &db).await?;

    Ok((created, id))
}

pub async fn index(page_req: PageReq, user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let media_w_titles = Media::find().filter(media::Column::UserId.eq(user.id)).find_also_related(Titles)
        .filter(titles::Column::Primary.eq(true))
        .order_by_asc(functions::default_media_sort())
        .offset(page_req.offset).limit(page_req.limit).all(db).await?;
    let indexes = build_media_indexes(media_w_titles);
    Ok(indexes)
}

pub async fn search(query: SearchQuery, committed: bool, page_req: PageReq, media_type: Option<MediaType>, user: &CurrentUser, db: &DbConn) -> Result<SearchResults, SrvErr> {
    let limit = page_req.limit;
    let res = transpiler::transpile(query, page_req, user, media_type.map(|m| { m.into() }));

    if res.is_err() {
        return Ok(SearchResults {
            indexes: vec![],
            external: vec![],
            query_valid: false,
        })
    }
    let res = res.ok().unwrap();
    let media_w_titles = res.query.all(db).await?;

    let external_t;
    let mut external_limit = 0;
    if res.is_primitive && (res.name_search.len() > 3 || committed || res.external_id.is_some()) {
        if let Some(l) = limit {
            if (l as usize) > media_w_titles.len() {
                external_limit = (l as usize) - media_w_titles.len();
            }
        } else {
            external_limit = usize::MAX;
        }
        if external_limit > 0 {
            if let Some(external_id) = res.external_id {
                match external_id.r#type {
                    MediaType::Movie => {
                        if media_w_titles.iter().any(|y| y.0.tmdb_id == Some(external_id.tmdb_id) && y.0.r#type == sea_orm_active_enums::MediaType::Movie) {
                            external_t = None;
                        } else {
                            let res = integrations::tmdb::services::movies::details(external_id.tmdb_id).map_ok(|x|
                                if !media_w_titles.iter().any(|y| y.0.tmdb_id == Some(x.id) && y.0.r#type == sea_orm_active_enums::MediaType::Movie) {
                                    vec![x.into_view()]
                                } else { Vec::new() }
                            ).await;
                            match res {
                                Ok(v) => { external_t = Some(v); },
                                Err(e) => {
                                    if e.status_code.is_some() && e.status_code == Some(StatusCode::NOT_FOUND) {
                                        external_t = None;
                                    } else { return Err(e.into()); }
                                }
                            }
                        }
                    }
                    MediaType::Series => {
                        if media_w_titles.iter().any(|y| y.0.tmdb_id == Some(external_id.tmdb_id) && y.0.r#type == sea_orm_active_enums::MediaType::Series) {
                            external_t = None;
                        } else {
                            let res = integrations::tmdb::services::series::details(external_id.tmdb_id).map_ok(|x|
                                if !media_w_titles.iter().any(|y| y.0.tmdb_id == Some(x.id) && y.0.r#type == sea_orm_active_enums::MediaType::Series) {
                                    vec![x.into_view()]
                                } else { Vec::new() }
                            ).await;
                            match res {
                                Ok(v) => { external_t = Some(v); },
                                Err(e) => {
                                    if e.status_code.is_some() && e.status_code == Some(StatusCode::NOT_FOUND) {
                                        external_t = None;
                                    } else { return Err(e.into()); }
                                }
                            }
                        }
                    }
                }
            } else {
                external_t = Some(
                    integrations::tmdb::services::search::multi(res.name_search).map_ok(|res| {
                        res.results.iter().filter_map(|r| {
                            if external_limit == 0 { return None; }
                            match r {
                                MultiResult::Movie(movie) => {
                                    if media_w_titles.iter().find(|x| {
                                        if x.0.tmdb_id == Some(movie.id) && x.0.r#type == sea_orm_active_enums::MediaType::Movie { true }
                                        else { false }
                                    }).is_some() { return None }
                                    if media_type.is_none() || media_type == Some(MediaType::Movie) { external_limit -= 1; Some(movie.into_view()) }
                                    else { None }
                                }
                                MultiResult::Tv(tv) => {
                                    if media_w_titles.iter().find(|x| {
                                        if x.0.tmdb_id == Some(tv.id) && x.0.r#type == sea_orm_active_enums::MediaType::Series { true }
                                        else { false }
                                    }).is_some() { return None }
                                    if media_type.is_none() || media_type == Some(MediaType::Series) { external_limit -= 1; Some(tv.into_view()) }
                                    else { None }
                                }
                                MultiResult::Person(_) => { None }
                                MultiResult::Collection(_) => { None }
                            }
                        }).collect()
                    }).await?
                );
            }
        } else { external_t = None }
    } else { external_t = None }

    let external;
    if let Some(e) = external_t {
        external = e;
    } else { external = Vec::new(); }

    let indexes = build_media_indexes(media_w_titles);

    Ok(SearchResults {
        indexes,
        external,
        query_valid: true
    })
}

pub async fn on_watchlist(media_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<bool, SrvErr> {
    let found = media::Entity::find_by_id(media_id).filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into())).count(db).await? > 0;

    if !found {
        return Err(SrvErr::NotFound)
    }

    let on_watchlist = watchlist::Entity::find().filter(watchlist::Column::MediaId.eq(media_id)).count(db).await? > 0;
    Ok(on_watchlist)
}

pub async fn backdrops(media_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<Vec<Image>, SrvErr> {
    let media = fetch_media(media_id, media_type, user, db).await?;

    let tmdb_images = if let Some(tmdb_id) = media.tmdb_id {
        if media_type == MediaType::Movie {
            tmdb::services::movies::images(tmdb_id, &media.original_language).await?
        } else {
            tmdb::services::series::images(tmdb_id, &media.original_language).await?
        }
    } else {
        return Ok(vec![])
    };

    let lang_codes = tmdb_images.backdrops.iter().filter_map(|x| x.iso_639_1.as_ref());

    let languages = Languages::find().filter(languages::Column::Iso6391.is_in(lang_codes)).all(db).await?;

    let mut backdrops: Vec<Image> = tmdb_images.backdrops.into_iter().map(|x| x.into_image(&media.backdrop_path, &languages)).collect();

    backdrops.sort_by(|y, x| x.sort_compare(y));

    Ok(backdrops)
}

pub async fn update_backdrop(media_id: i32, media_type: MediaType, backdrop: BackdropUpdate, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = fetch_media(media_id, media_type, user, db).await?;

    let mut am = media.into_active_model();
    am.backdrop_path = Set(Some(backdrop.path));

    am.update(db).await?;
    Ok(())
}

pub async fn posters(media_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<Vec<Image>, SrvErr> {
    let media = fetch_media(media_id, media_type, user, db).await?;

    let tmdb_images = if let Some(tmdb_id) = media.tmdb_id {
        if media_type == MediaType::Movie {
            tmdb::services::movies::images(tmdb_id, &media.original_language).await?
        } else {
            tmdb::services::series::images(tmdb_id, &media.original_language).await?
        }
    } else {
        return Ok(vec![])
    };

    let lang_codes = tmdb_images.posters.iter().filter_map(|x| x.iso_639_1.as_ref());

    let languages = Languages::find().filter(languages::Column::Iso6391.is_in(lang_codes)).all(db).await?;

    let mut posters: Vec<Image> = tmdb_images.posters.into_iter().map(|x| x.into_image(&media.poster_path, &languages)).collect();

    posters.sort_by(|y, x| x.sort_compare(y));

    Ok(posters)
}

pub async fn update_poster(media_id: i32, media_type: MediaType, poster: PosterUpdate, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = fetch_media(media_id, media_type, user, db).await?;

    let mut am = media.into_active_model();
    am.poster_path = Set(Some(poster.path));

    am.update(db).await?;
    Ok(())
}


pub async fn images(media_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<Images, SrvErr> {
    let media = fetch_media(media_id, media_type, user, db).await?;

    let tmdb_images = if let Some(tmdb_id) = media.tmdb_id {
        if media_type == MediaType::Movie {
            tmdb::services::movies::images(tmdb_id, &media.original_language).await?
        } else {
            tmdb::services::series::images(tmdb_id, &media.original_language).await?
        }
    } else {
        return Ok(Images {
            backdrops: vec![],
            posters: vec![],
        })
    };

    let lang_codes = tmdb_images.backdrops.iter().filter_map(|x| x.iso_639_1.as_ref())
        .chain(tmdb_images.posters.iter().filter_map(|x| x.iso_639_1.as_ref()));

    let languages = Languages::find().filter(languages::Column::Iso6391.is_in(lang_codes)).all(db).await?;

    let mut images = Images {
        backdrops: tmdb_images.backdrops.into_iter().map(|x| x.into_image(&media.backdrop_path, &languages)).collect(),
        posters: tmdb_images.posters.into_iter().map(|x| x.into_image(&media.poster_path, &languages)).collect(),
    };

    images.backdrops.sort_by(|y, x| x.sort_compare(y));
    images.posters.sort_by(|y, x| x.sort_compare(y));

    Ok(images)
}

pub async fn update_images(media_id: i32, media_type: MediaType, images: ImagesUpdate, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = fetch_media(media_id, media_type, user, db).await?;

    let mut am = media.into_active_model();
    if let Some(backdrop) = images.backdrop_path {
        am.backdrop_path = Set(Some(backdrop));
    }
    if let Some(poster) = images.poster_path {
        am.poster_path = Set(Some(poster));
    }
    am.update(db).await?;
    Ok(())
}


pub async fn delete_w_source(tmdb_id: i32, source_name: String, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = media::Entity::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::TmdbId.eq(tmdb_id))
        .filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into())).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();


    if user.though_bot && !media.bot_controllable {
        let source = media.find_related(Sources).filter(entities::sources::Column::Name.eq(source_name)).one(db).await?;
        if source.is_none() {
            return Err(SrvErr::NotFound);
        }
        let source = source.unwrap();

        delete_from_media(media, source, user, db).await?;

        return Ok(());
    }

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

pub async fn delete(media_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = fetch_media(media_id, media_type, user, db).await?;

    if user.though_bot && !media.bot_controllable {
        return Err(SrvErr::Unauthorized);
    }

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


pub(crate) fn build_media_indexes(media_w_titles: Vec<(media::Model, Option<titles::Model>)>) -> Vec<MediaIndex> {
    let mut indexes = Vec::with_capacity(media_w_titles.len());
    for m in media_w_titles {
        indexes.push(build_media_index(m));
    }

    indexes
}

pub(crate) fn build_media_index(m: (media::Model, Option<titles::Model>)) -> MediaIndex {
    let title = if let Some(title) = m.1 {
        title.title
    } else { env::var("UNSET_MEDIA_TITLE").expect("UNSET_MEDIA_TITLE not set!") };

    MediaIndex {
        id: m.0.id,
        r#type: views::media::MediaType::from(m.0.r#type),
        poster_path: m.0.poster_path,
        stars: m.0.stars,
        title,
    }
}

async fn fetch_media(media_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<media::Model, SrvErr> {
    let media = Media::find().filter(media::Column::Id.eq(media_id))
        .filter(media::Column::UserId.eq(user.id)).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into())).one(db).await?;

    media.ok_or(SrvErr::NotFound)
}