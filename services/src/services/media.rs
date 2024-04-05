use std::cmp::Ordering;
use std::env;

use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, IntoActiveModel, ModelTrait, PaginatorTrait, QueryOrder, TransactionTrait};
use sea_orm::ActiveValue::Set;
use sea_orm::prelude::Expr;
use sea_orm::QueryFilter;
use sea_orm::sea_query::Query;

use entities::{genres, languages, media, media_genres, media_tags, sea_orm_active_enums, tags, titles, watchlist};
use entities::prelude::{Genres, Languages, Media, MediaGenres, MediaTags, Sources, Tags, Titles};
use integrations::tmdb;
use integrations::tmdb::views::MultiResult;
use views::images::{Image, Images, ImagesUpdate};
use views::media::{MediaIndex, MediaSourceCreate, MediaType, SearchQuery, SearchResults};
use views::users::CurrentUser;

use crate::{movies, series, sources};
use crate::infrastructure::SrvErr;
use crate::infrastructure::traits::IntoView;
use crate::sources::delete_from_media;

pub async fn create_w_source(view: MediaSourceCreate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(bool, i32), SrvErr> {
    let (created, id) = match media_type {
        MediaType::Movie => { movies::create(view.tmdb_id, user, db).await? }
        MediaType::Series => { series::create(view.tmdb_id, user, db).await? }
    };

    sources::create(id, &view.source, media_type, &user, &db).await?;

    Ok((created, id))
}

pub async fn index(user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let media_w_titles = Media::find().filter(media::Column::UserId.eq(user.id)).find_also_related(Titles)
        .filter(titles::Column::Primary.eq(true)).order_by_asc(titles::Column::Title).all(db).await?;
    let indexes = build_media_indexes(media_w_titles, true);
    Ok(indexes)
}

pub async fn search(query: SearchQuery, committed: bool, media_type: Option<MediaType>, user: &CurrentUser, db: &DbConn) -> Result<SearchResults, SrvErr> {
    let res = transpiler::transpile(query, user, media_type.map(|m| { m.into() }));

    if res.is_err() {
        return Ok(SearchResults {
            indexes: vec![],
            external: vec![],
            query_valid: false,
        })
    }
    let res = res.ok().unwrap();

    let external_t;
    if res.is_primitive && (res.name_search.len() > 3 || committed) {
        external_t = Some(integrations::tmdb::services::search::multi(res.name_search));
    } else { external_t = None }
    let media_w_titles = res.query.all(db).await?;

    let external;
    if let Some(future) = external_t {
        external = future.await?.results.iter().filter_map(|r| {
            match r {
                MultiResult::Movie(movie) => {
                    if media_w_titles.iter().find(|x| {
                        if x.0.tmdb_id == Some(movie.id) && x.0.r#type == sea_orm_active_enums::MediaType::Movie { true }
                        else { false }
                    }).is_some() { return None }
                    if media_type.is_none() || media_type == Some(MediaType::Movie) { Some(movie.into_view()) }
                    else { None }
                }
                MultiResult::Tv(tv) => {
                    if media_w_titles.iter().find(|x| {
                        if x.0.tmdb_id == Some(tv.id) && x.0.r#type == sea_orm_active_enums::MediaType::Series { true }
                        else { false }
                    }).is_some() { return None }
                    if media_type.is_none() || media_type == Some(MediaType::Series) { Some(tv.into_view()) }
                    else { None }
                }
                MultiResult::Person(_) => { None }
            }
        }).collect();
    } else { external = Vec::new(); }

    let indexes = build_media_indexes(media_w_titles, !res.custom_sort);

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

pub async fn images(media_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<Images, SrvErr> {
    let media = Media::find().filter(media::Column::Id.eq(media_id))
        .filter(media::Column::UserId.eq(user.id)).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into())).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

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
        backdrops: tmdb_images.backdrops.into_iter().map(|x| {
            Image {
                language: x.iso_639_1.map(|z| languages.iter().find(|y| y.iso6391 == z).map(|l| l.english_name.clone())).flatten(),
                width: x.width,
                height: x.height,
                current: media.backdrop_path.as_ref().is_some_and(|p| p == &x.file_path),
                file_path: x.file_path,
            }
        }).collect(),
        posters: tmdb_images.posters.into_iter().map(|x| {
            Image {
                language: x.iso_639_1.map(|z| languages.iter().find(|y| y.iso6391 == z).map(|l| l.english_name.clone())).flatten(),
                width: x.width,
                height: x.height,
                current: media.poster_path.as_ref().is_some_and(|p| p == &x.file_path),
                file_path: x.file_path,
            }
        }).collect(),
    };

    images.backdrops.sort_by(|y, x| {
        if x.current != y.current {
            if x.current { Ordering::Greater }
            else { Ordering::Less }
        } else {
            (x.width * x.height).cmp(&(y.width * y.height))
        }
    });
    images.posters.sort_by(|y, x| {
        if x.current != y.current {
            if x.current { Ordering::Greater }
            else { Ordering::Less }
        } else {
            (x.width * x.height).cmp(&(y.width * y.height))
        }
    });

    Ok(images)
}

pub async fn update_images(media_id: i32, media_type: MediaType, images: ImagesUpdate, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = Media::find().filter(media::Column::Id.eq(media_id))
        .filter(media::Column::UserId.eq(user.id)).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into())).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

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
    let media = media::Entity::find_by_id(media_id).filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into())).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();


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


pub(crate) fn build_media_indexes(media_w_titles: Vec<(media::Model, Option<titles::Model>)>, sort: bool) -> Vec<MediaIndex> {
    let mut indexes = Vec::with_capacity(media_w_titles.len());
    for m in media_w_titles {
        indexes.push(build_media_index(m));
    }

    if sort {
        indexes.sort_by(|x, y| {
            let t1l = x.title.to_lowercase();
            let t1 = if t1l.starts_with("the ") {
                t1l.trim_start_matches("the ")
            } else if t1l.starts_with("a ") {
                t1l.trim_start_matches("a ")
            } else { t1l.as_str() };

            let t2l = y.title.to_lowercase();
            let t2 = if t2l.starts_with("the ") {
                t2l.trim_start_matches("the ")
            } else if t2l.starts_with("a ") {
                t2l.trim_start_matches("a ")
            } else { t2l.as_str() };
            t1.cmp(t2)
        });
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