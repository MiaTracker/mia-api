use entities::prelude::{Genres, ImageSizes, Images, Languages, ManualImageReferences, Media, MediaGenres, MediaLocks, MediaTags, Sources, Tags, Titles};
use entities::sea_orm_active_enums::{ImageFileType, ImageSource, ImageType};
use entities::traits::linked::MediaPosters;
use entities::traits::locks::SetLock;
use entities::{functions, genres, image_sizes, images, languages, manual_image_references, media, media_genres, media_locks, media_tags, sea_orm_active_enums, tags, titles, watchlist};
use futures::TryFutureExt;
use http::StatusCode;
use infrastructure::config;
use integrations::tmdb::views::{MultiMovieResult, MultiResult, MultiTvResult, PropertyChanges};
use sea_orm::prelude::Expr;
use sea_orm::sea_query::Query;
use sea_orm::ActiveValue::Set;
use sea_orm::QueryFilter;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, IntoActiveModel, ModelTrait, PaginatorTrait, QueryOrder, QuerySelect, TransactionTrait};
use std::collections::HashMap;
use std::path::Path;
use views::images::{BackdropUpdate, Image, ImageCandidate, ImageCandidates, ImageSize, ImagesUpdate, PosterUpdate};
use views::media::{InternalSearchResults, MediaByIdsQuery, MediaIndex, MediaSourceCreate, MediaType, PageReq, SearchQuery, SearchResults};
use views::users::CurrentUser;

use crate::images::{delete_unused_image, save_tmdb_image};
use crate::infrastructure::constants::tmdb_config;
use crate::infrastructure::traits::{IntoImage, IntoView, SortCompare};
use crate::infrastructure::{Clock, ImageStore, Random, SrvErr, Tmdb};
use crate::jobs::Jobs;
use crate::sources::delete_from_media;
use crate::{movies, series, sources};
use crate::genres::add_genre_to_media;

#[allow(clippy::too_many_arguments)]
pub async fn create_w_source(view: MediaSourceCreate, media_type: MediaType, user: &CurrentUser, db: &DbConn, tmdb: &dyn Tmdb, store: &dyn ImageStore, jobs: &dyn Jobs, clock: &dyn Clock, random: &dyn Random) -> Result<(bool, i32), SrvErr> {
    let (created, id) = match media_type {
        MediaType::Movie => { movies::create(view.tmdb_id, user, db, tmdb, store, jobs, clock, random).await? }
        MediaType::Series => { series::create(view.tmdb_id, user, db, tmdb, store, jobs, clock, random).await? }
    };

    sources::create(id, &view.source, media_type, &user, &db).await?;

    Ok((created, id))
}

pub async fn index(page_req: PageReq, user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let media = Media::find().filter(media::Column::UserId.eq(user.id))
        .find_also_linked(MediaPosters)
        .find_also_related(Titles)
        .filter(titles::Column::Primary.eq(true))
        .order_by_asc(functions::default_media_sort())
        .offset(page_req.offset).limit(page_req.limit).all(db).await?;

    let poster_ids: Vec<i32> = media.iter().filter_map(|p| p.0.poster_image_id).collect();
    let poster_sizes = ImageSizes::find()
        .filter(image_sizes::Column::ImageId.is_in(poster_ids)).all(db).await?;

    let indexes = build_media_indexes(media, poster_sizes);
    Ok(indexes)
}

pub async fn search_by_ids(req: MediaByIdsQuery, page_req: PageReq, user: &CurrentUser, db: &DbConn) -> Result<InternalSearchResults, SrvErr> {
    let ids = req.ids.clone();

    let res = transpiler::transpile(req.into(), page_req, user, None);
    let Ok(res) = res else {
        return Ok(InternalSearchResults {
            indexes: vec![],
            query_valid: false,
        });
    };

    if ids.is_empty() {
        return Ok(InternalSearchResults {
            indexes: vec![],
            query_valid: true,
        });
    }

    let media = res.query
        .filter(media::Column::Id.is_in(ids))
        .all(db).await?;

    let poster_ids: Vec<i32> = media.iter().filter_map(|p| p.0.poster_image_id).collect();
    let poster_sizes = ImageSizes::find()
        .filter(image_sizes::Column::ImageId.is_in(poster_ids)).all(db).await?;

    let indexes = build_media_indexes(media, poster_sizes);
    Ok(InternalSearchResults {
        indexes,
        query_valid: true
    })
}

/// The library half of a search: the page of rows plus what the transpiler learned about the query.
pub(crate) struct LibrarySearch {
    pub rows: Vec<(media::Model, Option<images::Model>, Option<titles::Model>)>,
    pub name_search: String,
    pub external_id: Option<transpiler::ExternalId>,
    pub is_primitive: bool,
}

/// Transpiles and runs a search query against the user's library. The inner `Err` holds one
/// `"{source}: {message}"` string per transpilation error; no query is run then.
pub(crate) async fn search_library(query: SearchQuery, page_req: PageReq, media_type: Option<MediaType>, user: &CurrentUser, db: &DbConn) -> Result<Result<LibrarySearch, Vec<String>>, SrvErr> {
    let res = match transpiler::transpile(query, page_req, user, media_type.map(|m| m.into())) {
        Ok(res) => res,
        Err(errs) => return Ok(Err(errs.into_iter().map(|e| format!("{}: {}", e.source, e.message)).collect())),
    };
    let rows = res.query.all(db).await?;

    Ok(Ok(LibrarySearch {
        rows,
        name_search: res.name_search,
        external_id: res.external_id,
        is_primitive: res.is_primitive,
    }))
}

/// A movie or series TMDB found; people and collections are never kept.
#[derive(Clone, Debug)]
pub(crate) enum TmdbHit {
    Movie(MultiMovieResult),
    Tv(MultiTvResult),
}

impl TmdbHit {
    pub(crate) fn tmdb_id(&self) -> i32 {
        match self {
            TmdbHit::Movie(m) => m.id,
            TmdbHit::Tv(t) => t.id,
        }
    }

    pub(crate) fn media_type(&self) -> MediaType {
        match self {
            TmdbHit::Movie(_) => MediaType::Movie,
            TmdbHit::Tv(_) => MediaType::Series,
        }
    }
}

/// Searches TMDB by name, keeping movies and series (only `media_type` when given) in TMDB's order.
/// The raw TMDB error is returned so each caller decides how to report it.
pub(crate) async fn search_tmdb(query: String, media_type: Option<MediaType>, tmdb: &dyn Tmdb) -> Result<Vec<TmdbHit>, integrations::infrastructure::Error> {
    let results = tmdb.search_multi(query).await?;
    Ok(results.results.into_iter().filter_map(|r| match r {
        MultiResult::Movie(movie) => Some(TmdbHit::Movie(movie)),
        MultiResult::Tv(tv) => Some(TmdbHit::Tv(tv)),
        MultiResult::Person(_) | MultiResult::Collection(_) => None,
    }).filter(|hit| media_type.is_none() || media_type == Some(hit.media_type())).collect())
}

pub async fn search(query: SearchQuery, committed: bool, page_req: PageReq, media_type: Option<MediaType>, user: &CurrentUser, db: &DbConn, tmdb: &dyn Tmdb) -> Result<SearchResults, SrvErr> {
    let limit = page_req.limit;
    let Ok(library) = search_library(query, page_req, media_type, user, db).await? else {
        return Ok(SearchResults {
            indexes: vec![],
            external: vec![],
            query_valid: false,
        })
    };
    let LibrarySearch { rows: media, name_search, external_id, is_primitive } = library;

    let poster_ids: Vec<i32> = media.iter().filter_map(|p| p.0.poster_image_id).collect();
    let poster_sizes = ImageSizes::find()
        .filter(image_sizes::Column::ImageId.is_in(poster_ids)).all(db).await?;

    let external_t;
    let mut external_limit = 0;
    if is_primitive && (name_search.len() > 3 || committed || external_id.is_some()) {
        if let Some(l) = limit {
            if (l as usize) > media.len() {
                external_limit = (l as usize) - media.len();
            }
        } else {
            external_limit = usize::MAX;
        }
        if external_limit > 0 {
            if let Some(external_id) = external_id {
                match external_id.r#type {
                    MediaType::Movie => {
                        if media.iter().any(|y| y.0.tmdb_id == Some(external_id.tmdb_id) && y.0.r#type == sea_orm_active_enums::MediaType::Movie) {
                            external_t = None;
                        } else {
                            let res = tmdb.movie_details(external_id.tmdb_id).map_ok(|x|
                                if !media.iter().any(|y| y.0.tmdb_id == Some(x.id) && y.0.r#type == sea_orm_active_enums::MediaType::Movie) {
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
                        if media.iter().any(|y| y.0.tmdb_id == Some(external_id.tmdb_id) && y.0.r#type == sea_orm_active_enums::MediaType::Series) {
                            external_t = None;
                        } else {
                            let res = tmdb.series_details(external_id.tmdb_id).map_ok(|x|
                                if !media.iter().any(|y| y.0.tmdb_id == Some(x.id) && y.0.r#type == sea_orm_active_enums::MediaType::Series) {
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
                let owned = |hit: &TmdbHit| media.iter().any(|x| x.0.tmdb_id == Some(hit.tmdb_id()) && x.0.r#type == hit.media_type().into());
                external_t = Some(
                    search_tmdb(name_search, media_type, tmdb).await?
                        .into_iter()
                        .filter(|hit| !owned(hit))
                        .take(external_limit)
                        .map(|hit| match hit {
                            TmdbHit::Movie(movie) => (&movie).into_view(),
                            TmdbHit::Tv(tv) => (&tv).into_view(),
                        })
                        .collect()
                );
            }
        } else { external_t = None }
    } else { external_t = None }

    let external;
    if let Some(e) = external_t {
        external = e;
    } else { external = Vec::new(); }

    let indexes = build_media_indexes(media, poster_sizes);

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

pub async fn backdrops(media_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn, tmdb: &dyn Tmdb) -> Result<Vec<ImageCandidate>, SrvErr> {
    let media = fetch_media(media_id, media_type, user, db).await?;

    let tmdb_images = if let Some(tmdb_id) = media.tmdb_id {
        if media_type == MediaType::Movie {
            tmdb.movie_images(tmdb_id, &media.original_language).await?
        } else {
            tmdb.series_images(tmdb_id, &media.original_language).await?
        }
    } else {
        return Ok(vec![])
    };

    let lang_codes = tmdb_images.backdrops.iter().filter_map(|x| x.iso_639_1.as_ref());

    let languages = Languages::find().filter(languages::Column::Iso6391.is_in(lang_codes)).all(db).await?;

    let media_backdrop = if let Some(backdrop_id) = media.backdrop_image_id {
        Images::find_by_id(backdrop_id)
            .find_with_related(ImageSizes)
            .all(db).await?.into_iter().next()
    } else {
        None
    };

    let manual_backdrops = Images::find().inner_join(ManualImageReferences)
        .filter(images::Column::Type.eq(ImageType::Backdrop))
        .filter(manual_image_references::Column::MediaId.eq(media.id))
        .find_with_related(ImageSizes)
        .all(db).await?;

    let mut backdrops: Vec<ImageCandidate> = media_backdrop.as_ref().map_or_else(Vec::new, |backdrop| {
        vec![
            backdrop.into_view().map(|mut b| {
                b.current = true;
                b
            })]
        }).into_iter()
        .chain(manual_backdrops.into_iter().map(|x| x.into_view()))
        .chain(
            tmdb_images.backdrops.into_iter().filter_map(|x|
                if let Some(backdrop) = &media_backdrop && backdrop.0.source == ImageSource::Tmdb && backdrop.0.source_path.as_ref().is_some_and(|p| p == &x.file_path) {
                    None
                }
                else {
                    Some(Ok(x.into_image(&languages, &tmdb_config().images.backdrop_sizes)))
                }
            )
        ).collect::<Result<Vec<ImageCandidate>, SrvErr>>()?;


    backdrops.sort_by(|y, x| x.sort_compare(y));

    Ok(backdrops)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_backdrop(media_id: i32, media_type: MediaType, backdrop: BackdropUpdate, user: &CurrentUser, db: &DbConn, tmdb: &dyn Tmdb, store: &dyn ImageStore, jobs: &dyn Jobs, random: &dyn Random) -> Result<(), SrvErr> {
    let media = fetch_media(media_id, media_type, user, db).await?;
    let image_id = fetch_id_or_pull_image(backdrop.source, backdrop.path.as_str(), ImageType::Backdrop, db, tmdb, store, jobs, random).await?;
    let old_image_id = media.backdrop_image_id;
    let mut am = media.into_active_model();
    am.backdrop_image_id = Set(Some(image_id));

    am.update(db).await?;

    if let Some(old_image_id) = old_image_id {
        delete_unused_image(old_image_id, db, store).await?;
    }

    Ok(())
}

pub async fn posters(media_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn, tmdb: &dyn Tmdb) -> Result<Vec<ImageCandidate>, SrvErr> {
    let media = fetch_media(media_id, media_type, user, db).await?;

    let tmdb_images = if let Some(tmdb_id) = media.tmdb_id {
        if media_type == MediaType::Movie {
            tmdb.movie_images(tmdb_id, &media.original_language).await?
        } else {
            tmdb.series_images(tmdb_id, &media.original_language).await?
        }
    } else {
        return Ok(vec![])
    };

    let lang_codes = tmdb_images.posters.iter().filter_map(|x| x.iso_639_1.as_ref());

    let languages = Languages::find().filter(languages::Column::Iso6391.is_in(lang_codes)).all(db).await?;

    let media_poster = if let Some(poster_id) = media.poster_image_id {
        Images::find_by_id(poster_id)
            .find_with_related(ImageSizes)
            .all(db).await?.into_iter().next()
    } else {
        None
    };

    let manual_posters = Images::find().inner_join(ManualImageReferences)
        .filter(images::Column::Type.eq(ImageType::Poster))
        .filter(manual_image_references::Column::MediaId.eq(media.id))
        .find_with_related(ImageSizes)
        .all(db).await?;

    let mut posters: Vec<ImageCandidate> = media_poster.as_ref().map_or_else(Vec::new, |poster| {
        vec![
            poster.into_view().map(|mut p| {
                p.current = true;
                p
            })
        ]}).into_iter()
        .chain(
            manual_posters.into_iter().map(|x| x.into_view())
        ).chain(
        tmdb_images.posters.into_iter().filter_map(|x|
            if let Some(poster) = &media_poster && poster.0.source == ImageSource::Tmdb && poster.0.source_path.as_ref().is_some_and(|p| p == &x.file_path) {
                None
            }
            else {
                Some(Ok(x.into_image(&languages, &tmdb_config().images.poster_sizes)))
            }
        )
    ).collect::<Result<Vec<ImageCandidate>, SrvErr>>()?;

    posters.sort_by(|y, x| x.sort_compare(y));

    Ok(posters)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_poster(media_id: i32, media_type: MediaType, poster: PosterUpdate, user: &CurrentUser, db: &DbConn, tmdb: &dyn Tmdb, store: &dyn ImageStore, jobs: &dyn Jobs, random: &dyn Random) -> Result<(), SrvErr> {
    let media = fetch_media(media_id, media_type, user, db).await?;
    let image_id = fetch_id_or_pull_image(poster.source, poster.path.as_str(), ImageType::Poster, db, tmdb, store, jobs, random).await?;
    let old_image_id = media.poster_image_id;

    let mut am = media.into_active_model();
    am.poster_image_id = Set(Some(image_id));

    am.update(db).await?;

    if let Some(old_image_id) = old_image_id {
        delete_unused_image(old_image_id, db, store).await?;
    }

    Ok(())
}


pub async fn images(media_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn, tmdb: &dyn Tmdb) -> Result<ImageCandidates, SrvErr> {
    let media = fetch_media(media_id, media_type, user, db).await?;

    let tmdb_images = if let Some(tmdb_id) = media.tmdb_id {
        if media_type == MediaType::Movie {
            tmdb.movie_images(tmdb_id, &media.original_language).await?
        } else {
            tmdb.series_images(tmdb_id, &media.original_language).await?
        }
    } else {
        return Ok(ImageCandidates {
            backdrops: vec![],
            posters: vec![],
        })
    };

    let lang_codes = tmdb_images.backdrops.iter().filter_map(|x| x.iso_639_1.as_ref())
        .chain(tmdb_images.posters.iter().filter_map(|x| x.iso_639_1.as_ref()));

    let languages = Languages::find().filter(languages::Column::Iso6391.is_in(lang_codes)).all(db).await?;

    let mut images = ImageCandidates {
        backdrops: tmdb_images.backdrops.into_iter().map(|x| x.into_image(&languages, &tmdb_config().images.backdrop_sizes)).collect(),
        posters: tmdb_images.posters.into_iter().map(|x| x.into_image(&languages, &tmdb_config().images.poster_sizes)).collect(),
    };

    images.backdrops.sort_by(|y, x| x.sort_compare(y));
    images.posters.sort_by(|y, x| x.sort_compare(y));

    Ok(images)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_images(media_id: i32, media_type: MediaType, images: ImagesUpdate, user: &CurrentUser, db: &DbConn, tmdb: &dyn Tmdb, store: &dyn ImageStore, jobs: &dyn Jobs, random: &dyn Random) -> Result<(), SrvErr> {
    let media = fetch_media(media_id, media_type, user, db).await?;

    let mut am = media.into_active_model();
    if let Some(backdrop) = images.backdrop_path {
        let source = images.backdrop_source.ok_or(SrvErr::BadRequest("Source not provided".to_string()))?;
        let backdrop_image_id = fetch_id_or_pull_image(source, backdrop.as_str(), ImageType::Backdrop, db, tmdb, store, jobs, random).await?;
        am.backdrop_image_id = Set(Some(backdrop_image_id));
    }
    if let Some(poster) = images.poster_path {
        let source = images.poster_source.ok_or(SrvErr::BadRequest("Source not provided".to_string()))?;
        let poster_image_id = fetch_id_or_pull_image(source, poster.as_str(), ImageType::Poster, db, tmdb, store, jobs, random).await?;
        am.poster_image_id = Set(Some(poster_image_id));
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

pub async fn delete(media_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn, store: &dyn ImageStore) -> Result<(), SrvErr> {
    let media = fetch_media(media_id, media_type, user, db).await?;

    if user.though_bot && !media.bot_controllable {
        return Err(SrvErr::Unauthorized);
    }

    let poster_id = media.poster_image_id;
    let backdrop_id = media.backdrop_image_id;

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

    if let Some(poster_id) = poster_id {
        delete_unused_image(poster_id, db, store).await?;
    }

    if let Some(backdrop_id) = backdrop_id {
        delete_unused_image(backdrop_id, db, store).await?;
    }

    let deleted_references = ManualImageReferences::delete_many()
        .filter(manual_image_references::Column::MediaId.eq(media_id))
        .exec_with_returning(db).await?;

    for deleted_reference in deleted_references {
        delete_unused_image(deleted_reference.image_id, db, store).await?;
    }

    tran.commit().await?;

    Ok(())
}


pub(crate) fn build_media_indexes(data: Vec<(media::Model, Option<images::Model>, Option<titles::Model>)>, poster_sizes: Vec<image_sizes::Model>) -> Vec<MediaIndex> {
    let mut sizes_map: HashMap<i32, Vec<&image_sizes::Model>> = HashMap::new();

    for size in &poster_sizes {
        if let Some(vec) = sizes_map.get_mut(&size.image_id) {
            vec.push(size);
        }
        else {
            let vec = vec![size];
            sizes_map.insert(size.image_id.clone(), vec);
        }
    }

    let mut indexes = Vec::with_capacity(data.len());
    for m in data {
        let sizes = m.0.poster_image_id.and_then(|i| sizes_map.get(&i));
        indexes.push(build_media_index(m, sizes));
    }

    indexes
}

pub(crate) fn build_media_index(m: (media::Model, Option<images::Model>, Option<titles::Model>), poster_sizes: Option<&Vec<&image_sizes::Model>>) -> MediaIndex {
    let title = if let Some(title) = m.2 {
        title.title
    } else { config().media.unset_title.clone() };

    let image = m.1.map(|i| Image {
        path: format!("/{}.{}", i.path, i.file_type.to_extension()),
        sizes: poster_sizes.map_or(vec![], |v| v.iter().map(|s| ImageSize::from(*s)).collect())
    });

    MediaIndex {
        id: m.0.id,
        r#type: views::media::MediaType::from(m.0.r#type),
        poster: image,
        stars: m.0.stars,
        title,
    }
}

pub(crate) async fn try_set_media_lock(media: media::Model, property: &str, locked: bool, db: &DbConn) -> Result<bool, SrvErr> {
    if !media_locks::ActiveModel::has_lock(property) {
        return Ok(false);
    }

    let locks = media.find_related(MediaLocks).one(db).await?;
    if let Some(locks) = locks {
        let mut active_locks = locks.into_active_model();
        active_locks.set_lock(property, locked);
        active_locks.update(db).await?;
    }
    else {
        let mut active_locks = media_locks::ActiveModel {
            media_id: Set(media.id),
            backdrop_path: Set(false),
            homepage: Set(false),
            imdb_id: Set(false),
            overview: Set(false),
            poster_path: Set(false),
            tmdb_vote_average: Set(false),
            original_language: Set(false),
            origin_country: Set(false),
        };
        active_locks.set_lock(property, locked);
        active_locks.insert(db).await?;
    }
    Ok(true)
}

pub(crate) async fn fetch_media(media_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<media::Model, SrvErr> {
    let media = Media::find().filter(media::Column::Id.eq(media_id))
        .filter(media::Column::UserId.eq(user.id)).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into())).one(db).await?;

    media.ok_or(SrvErr::NotFound)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn fetch_id_or_pull_image(source: views::images::ImageSource, path: &str, image_type: ImageType, db: &DbConn, tmdb: &dyn Tmdb, store: &dyn ImageStore, jobs: &dyn Jobs, random: &dyn Random) -> Result<i32, SrvErr> {
    match source {
        views::images::ImageSource::Internal => {
            let backdrop_path = Path::new(&path);
            let path = backdrop_path.file_stem().map(|p| p.to_str())
                .ok_or(SrvErr::BadRequest("Invalid path".to_string()))?;
            let file_type = ImageFileType::try_from_extension(backdrop_path.extension()
                .ok_or(SrvErr::BadRequest("Invalid path".to_string()))?.to_str()
                .ok_or(SrvErr::BadRequest("Invalid path".to_string()))?)
                .ok_or(SrvErr::BadRequest("Invalid path".to_string()))?;
            let image = Images::find().filter(images::Column::Path.eq(path)
                .and(images::Column::Type.eq(image_type)
                    .and(images::Column::FileType.eq(file_type)))).one(db).await?;
            image.map(|i| i.id)
        }
        views::images::ImageSource::TMDB => {
            Some(save_tmdb_image(path, image_type, db, tmdb, store, jobs, random).await?)
        }
    }.ok_or(SrvErr::BadRequest("Invalid path".to_string()))
}

/// Apply TMDB property-level changes to the shared media table fields.
/// Only updates fields that changed AND are not locked.
/// Adds new titles as alternative titles (never touches the primary title).
pub async fn apply_changes(media: &media::Model, changes: &PropertyChanges, db: &DbConn, tmdb: &dyn Tmdb, store: &dyn ImageStore, jobs: &dyn Jobs, random: &dyn Random) -> Result<(), SrvErr> {
    let locks = media.find_related(MediaLocks).one(db).await?;

    let mut media_am = media::ActiveModel {
        id: Set(media.id),
        ..Default::default()
    };
    let mut any_change = false;


    async fn add_title(media_id: i32, title_str: &str, db: &DbConn) -> Result<(), SrvErr> {
        let existing = Titles::find()
            .filter(titles::Column::MediaId.eq(media_id))
            .filter(titles::Column::Title.eq(title_str))
            .count(db).await?;
        if existing == 0 {
            titles::ActiveModel {
                id: sea_orm::NotSet,
                media_id: Set(media_id),
                primary: Set(false),
                title: Set(title_str.to_string()),
            }.insert(db).await?;
        }
        Ok(())
    }

    for change in &changes.changes {
        let last_item = match change.items.last() {
            Some(item) => item,
            None => continue,
        };
        let value = match &last_item.value {
            Some(v) => v,
            None => continue,
        };

        match change.key.as_str() {
            "overview" => {
                let lang = last_item.iso_639_1.as_deref().unwrap_or("");
                if lang == "en" && !is_locked(&locks, |l| l.overview) {
                    media_am.overview = Set(value.as_str().map(|s| s.to_string()));
                    any_change = true;
                }
            }
            "homepage" => {
                if !is_locked(&locks, |l| l.homepage) {
                    media_am.homepage = Set(value.as_str().map(|s| s.to_string()));
                    any_change = true;
                }
            }
            "imdb_id" => {
                if !is_locked(&locks, |l| l.imdb_id) {
                    media_am.imdb_id = Set(value.as_str().map(|s| s.to_string()));
                    any_change = true;
                }
            }
            "iso_639_1" => {
                if !is_locked(&locks, |l| l.original_language) {
                    media_am.original_language = Set(value.as_str().map(|s| s.to_string()));
                    any_change = true;
                }
            }
            "origin_country" => {
                if !is_locked(&locks, |l| l.origin_country) {
                    if let Some(arr) = value.as_array() {
                        let countries: Vec<String> = arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect();
                        media_am.origin_country = Set(Some(countries));
                        any_change = true;
                    }
                }
            }
            "images" => {
                if media.poster_image_id.is_none() && !is_locked(&locks, |l| l.poster_path) {
                    let poster = change.items.iter().filter(|i| i.value.as_ref()
                        .is_some_and(|o| o.get("poster").is_some())).next()
                        .map(|v| v.value.as_ref().unwrap().get("poster").unwrap());

                    if let Some(poster) = poster {
                        if let Some(file_path) = poster.get("file_path").and_then(|v| v.as_str()) {
                        let new_id = save_tmdb_image(file_path, ImageType::Poster, db, tmdb, store, jobs, random).await?;
                            media_am.poster_image_id = Set(Some(new_id));
                            any_change = true;
                        }
                    }
                }
                if media.backdrop_image_id.is_none() && !is_locked(&locks, |l| l.backdrop_path) {
                    let backdrop = change.items.iter().filter(|i| i.value.as_ref()
                        .is_some_and(|o| o.get("backdrop").is_some())).next()
                        .map(|v| v.value.as_ref().unwrap().get("backdrop").unwrap());

                    if let Some(backdrop) = backdrop {
                        if let Some(file_path) = backdrop.get("file_path").and_then(|v| v.as_str()) {
                        let new_id = save_tmdb_image(file_path, ImageType::Backdrop, db, tmdb, store, jobs, random).await?;
                            media_am.backdrop_image_id = Set(Some(new_id));
                            any_change = true;
                        }
                    }
                }
            }
            "title" | "original_title" | "name" | "original_name" => {
                let lang = last_item.iso_639_1.as_deref().unwrap_or("");
                if lang == "en" || media.original_language.as_ref().map(|l| l == lang).unwrap_or(false) {
                    if let Some(title_str) = value.as_str() {
                        add_title(media.id, title_str, db).await?;
                    }
                }
            }
            "alternative_titles" => {
                let lang = last_item.iso_639_1.as_deref().unwrap_or("");
                if lang == "en" || media.original_language.as_ref().map(|l| l == lang).unwrap_or(false) {
                    if let Some(title_str) = value.get("title").and_then(|t| t.as_str()) {
                        add_title(media.id, title_str, db).await?;
                    }
                }
            }
            "genres" => {
                if let Some(genre_str) = value.get("name").and_then(|n| n.as_str()) {
                    add_genre_to_media(media, &genre_str, db).await?;
                }
            }
            _ => {}
        }
    }

    if any_change {
        media_am.update(db).await?;
    }

    Ok(())
}

pub fn is_locked<T>(locks: &Option<T>, f: impl Fn(&T) -> bool) -> bool {
    locks.as_ref().map_or(false, f)
}

#[cfg(test)]
mod tests {
    use integrations::tmdb::views::{ChangeItem, MovieDetails as TmdbMovieDetails, MovieTitles, MultiPersonResult, SeriesDetails as TmdbSeriesDetails, SeriesTitles, TmdbImage, TmdbImages};
    use sea_orm::{MockDatabase, Value};
    use uuid::Uuid;
    use views::sources::{SourceCreate, SourceType};
    use super::*;
    use crate::test_support;
    use crate::test_support::db::{assert_no_statement_starting_with, assert_statement_count, assert_wrote, deleted, with_deleted_unused_image, with_saved_tmdb_image};
    use crate::test_support::fixtures;
    use crate::test_support::jobs::sizes;
    use crate::test_support::tmdb::{error, error_with_status, multi_movie, multi_results, multi_tv, png_bytes, property_changes, tmdb_images};
    use crate::test_support::{fixed_clock, InMemoryImageStore, MockTmdb, RecordingJobs, SeqRandom};

    // ---------------------------------------------------------------- helpers

    fn page(limit: Option<u64>) -> PageReq {
        PageReq { offset: None, limit }
    }

    fn search_query(query: &str) -> SearchQuery {
        SearchQuery { query: query.to_string(), genres: None, only_watched: false, min_stars: None, sort_by: Default::default() }
    }

    fn source_create(name: &str) -> SourceCreate {
        SourceCreate { name: name.to_string(), url: "https://example.test/new".to_string(), r#type: SourceType::Torrent }
    }

    /// Movie 10 (TMDB id 603, original language English) owned by user 1.
    fn tmdb_movie() -> media::Model {
        media::Model { tmdb_id: Some(603), original_language: Some("en".into()), ..fixtures::movie_media_model(10, 1) }
    }

    /// Series 10 (TMDB id 70, original language English) owned by user 1.
    fn tmdb_series() -> media::Model {
        media::Model { tmdb_id: Some(70), original_language: Some("en".into()), ..fixtures::series_media_model(10, 1) }
    }

    /// The row an owned movie with TMDB id `tmdb_id` has in a search result.
    fn owned_movie_row(id: i32, tmdb_id: i32, title: &str) -> std::collections::BTreeMap<String, Value> {
        let media = media::Model { tmdb_id: Some(tmdb_id), ..fixtures::movie_media_model(id, 1) };
        fixtures::media_index_row(media, None, Some(fixtures::title_model(id, id, title, true)))
    }

    /// A TMDB backdrop larger than `tmdb_images()`' one, in no language.
    fn large_backdrop() -> TmdbImage {
        TmdbImage { file_path: "/backdrop-2.jpg".into(), width: 3840, height: 2160, iso_639_1: None, ..tmdb_images().backdrops[0].clone() }
    }

    /// A TMDB poster larger than `tmdb_images()`' one, in no language.
    fn large_poster() -> TmdbImage {
        TmdbImage { file_path: "/poster-2.jpg".into(), width: 2000, height: 3000, iso_639_1: None, ..tmdb_images().posters[0].clone() }
    }

    /// `tmdb_images()` plus `large_backdrop()` and `large_poster()`.
    fn two_of_each_tmdb_images() -> TmdbImages {
        let mut images = tmdb_images();
        images.backdrops.push(large_backdrop());
        images.posters.push(large_poster());
        images
    }

    fn unused_deps() -> (MockTmdb, InMemoryImageStore, RecordingJobs) {
        (MockTmdb::new(), InMemoryImageStore::new(), RecordingJobs::new())
    }

    /// Runs `create_w_source` with a fixed clock and sequential randomness.
    async fn run_create_w_source(tmdb_id: i32, source: SourceCreate, media_type: MediaType, user: &CurrentUser, db: &DbConn, tmdb: &MockTmdb) -> Result<(bool, i32), SrvErr> {
        let view = MediaSourceCreate { tmdb_id, source };
        create_w_source(view, media_type, user, db, tmdb, &InMemoryImageStore::new(), &RecordingJobs::new(), &fixed_clock(2026, 3, 4, 12, 0, 0), &SeqRandom::new()).await
    }

    /// Runs `apply_changes` with sequential randomness.
    async fn run_apply_changes(media: &media::Model, changes: &PropertyChanges, db: &DbConn, tmdb: &MockTmdb, store: &InMemoryImageStore, jobs: &RecordingJobs) -> Result<(), SrvErr> {
        apply_changes(media, changes, db, tmdb, store, jobs, &SeqRandom::new()).await
    }

    /// Like `run_apply_changes`, with a TMDB, store and jobs that must not be used.
    async fn run_apply_changes_unused_deps(media: &media::Model, changes: &PropertyChanges, db: &DbConn) -> Result<(), SrvErr> {
        let (tmdb, store, jobs) = unused_deps();
        run_apply_changes(media, changes, db, &tmdb, &store, &jobs).await
    }

    /// Sets the language of every change item.
    fn with_language(mut changes: PropertyChanges, language: &str) -> PropertyChanges {
        for change in &mut changes.changes {
            for item in &mut change.items {
                item.iso_639_1 = Some(language.into());
            }
        }
        changes
    }

    /// An `images` change whose items carry a new poster, a new backdrop, or both.
    fn images_change(poster: Option<&str>, backdrop: Option<&str>) -> PropertyChanges {
        let mut changes = property_changes(&[("images", serde_json::json!({}))]);
        let template = changes.changes[0].items[0].clone();
        changes.changes[0].items.clear();
        if let Some(poster) = poster {
            changes.changes[0].items.push(ChangeItem { value: Some(serde_json::json!({"poster": {"file_path": poster}})), ..template.clone() });
        }
        if let Some(backdrop) = backdrop {
            changes.changes[0].items.push(ChangeItem { value: Some(serde_json::json!({"backdrop": {"file_path": backdrop}})), ..template });
        }
        changes
    }

    // ------------------------------------------------------------ create_w_source

    #[tokio::test]
    async fn create_w_source_creates_movie_and_source() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_details().withf(|id| *id == 603).times(1).returning(|id| Ok(TmdbMovieDetails {
            backdrop_path: None, poster_path: None, ..test_support::tmdb::movie_details(id)
        }));
        tmdb.expect_movie_alternative_titles().times(1).returning(|id| Ok(MovieTitles { id, titles: Vec::new() }));
        let db = test_support::db::mock()
            // movie: not added yet, then media, movie and title rows
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .append_query_results([[fixtures::movie_model(10)]])
            .append_query_results([[fixtures::title_model(1, 10, "Movie 603", true)]])
            // source: media lookup, name check, source row
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([[fixtures::source_model(3, 10, "Torrent A")]])
            .into_connection();

        let result = run_create_w_source(603, source_create("Torrent A"), MediaType::Movie, &fixtures::user(), &db, &tmdb).await;

        assert!(matches!(result, Ok((true, 10))));
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "movies""#, &[Value::from(10)]);
        assert_wrote(&log, r#"INSERT INTO "sources""#, &[Value::from(10), Value::from("Torrent A"), Value::from("https://example.test/new")]);
    }

    #[tokio::test]
    async fn create_w_source_creates_series_and_source() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_series_details().withf(|id| *id == 70).times(1).returning(|id| Ok(TmdbSeriesDetails {
            backdrop_path: None, poster_path: None, ..test_support::tmdb::series_details(id)
        }));
        tmdb.expect_series_alternative_titles().times(1).returning(|id| Ok(SeriesTitles { id, results: Vec::new() }));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([[fixtures::series_media_model(10, 1)]])
            .append_query_results([[fixtures::series_model(10)]])
            .append_query_results([[fixtures::title_model(1, 10, "Series 70", true)]])
            .append_query_results([[fixtures::series_media_model(10, 1)]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([[fixtures::source_model(3, 10, "Torrent A")]])
            .into_connection();

        let result = run_create_w_source(70, source_create("Torrent A"), MediaType::Series, &fixtures::user(), &db, &tmdb).await;

        assert!(matches!(result, Ok((true, 10))));
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "series""#, &[Value::from(10)]);
        assert_wrote(&log, r#"INSERT INTO "sources""#, &[Value::from(10), Value::from("Torrent A")]);
    }

    #[tokio::test]
    async fn create_w_source_adds_source_to_existing_media_when_already_added() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[media::Model { tmdb_id: Some(603), ..fixtures::movie_media_model(5, 1) }]])
            .append_query_results([[fixtures::movie_media_model(5, 1)]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([[fixtures::source_model(3, 5, "Torrent A")]])
            .into_connection();

        let result = run_create_w_source(603, source_create("Torrent A"), MediaType::Movie, &fixtures::user(), &db, &MockTmdb::new()).await;

        assert!(matches!(result, Ok((false, 5))));
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "sources""#, &[Value::from(5), Value::from("Torrent A")]);
        assert_no_statement_starting_with(&log, r#"INSERT INTO "media""#);
    }

    // ---------------------------------------------------------------------- index

    #[tokio::test]
    async fn index_returns_page() {
        test_support::init();
        let with_poster = media::Model { poster_image_id: Some(2), stars: Some(4.5), ..fixtures::movie_media_model(1, 1) };
        let without_poster = fixtures::series_media_model(3, 1);
        let db = test_support::db::mock()
            .append_query_results([vec![
                fixtures::media_index_row(with_poster, Some(fixtures::image_model(2)), Some(fixtures::title_model(1, 1, "Alien", true))),
                fixtures::media_index_row(without_poster, None, Some(fixtures::title_model(3, 3, "Bluey", true))),
            ]])
            .append_query_results([[fixtures::image_size_model(1, 2, 300, 450), fixtures::image_size_model(2, 2, 100, 150)]])
            .into_connection();

        let result = index(PageReq { offset: Some(20), limit: Some(10) }, &fixtures::user(), &db).await.ok().expect("index should succeed");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, 1);
        assert!(result[0].r#type == MediaType::Movie);
        assert_eq!(result[0].title, "Alien");
        assert_eq!(result[0].stars, Some(4.5));
        let poster = result[0].poster.as_ref().expect("first media has a poster");
        assert_eq!(poster.path, "/image-2.jpg");
        assert_eq!(poster.sizes.len(), 2);
        assert_eq!(result[1].id, 3);
        assert!(result[1].r#type == MediaType::Series);
        assert_eq!(result[1].title, "Bluey");
        assert!(result[1].poster.is_none());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_wrote(&log, "SELECT", &[Value::from(1), Value::from(20u64), Value::from(10u64)]);
    }

    // ---------------------------------------------------------------- search_by_ids

    #[tokio::test]
    async fn search_by_ids_returns_matching_media() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([vec![owned_movie_row(4, 603, "Alien"), owned_movie_row(6, 604, "Aliens")]])
            .append_query_results([Vec::<image_sizes::Model>::new()])
            .into_connection();
        let req = MediaByIdsQuery { query: String::new(), ids: vec![4, 6] };

        let result = search_by_ids(req, page(None), &fixtures::user(), &db).await.ok().expect("search should succeed");

        assert!(result.query_valid);
        assert_eq!(result.indexes.iter().map(|i| i.id).collect::<Vec<_>>(), vec![4, 6]);
        assert_eq!(result.indexes[1].title, "Aliens");
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_wrote(&log, "SELECT", &[Value::from(4), Value::from(6)]);
    }

    #[tokio::test]
    async fn search_by_ids_returns_empty_without_querying_when_no_ids() {
        test_support::init();
        let db = test_support::db::mock().into_connection();
        let req = MediaByIdsQuery { query: String::new(), ids: Vec::new() };

        let result = search_by_ids(req, page(None), &fixtures::user(), &db).await.ok().expect("search should succeed");

        assert!(result.query_valid);
        assert!(result.indexes.is_empty());
        assert_statement_count(&test_support::db::log(db), 0);
    }

    #[tokio::test]
    async fn search_by_ids_returns_invalid_query_when_query_unparseable() {
        test_support::init();
        let db = test_support::db::mock().into_connection();
        // The part after " : " is an expression; "@" is not a valid token.
        let req = MediaByIdsQuery { query: "alien : @".to_string(), ids: vec![4] };

        let result = search_by_ids(req, page(None), &fixtures::user(), &db).await.ok().expect("search should succeed");

        assert!(!result.query_valid);
        assert!(result.indexes.is_empty());
        assert_statement_count(&test_support::db::log(db), 0);
    }

    // ---------------------------------------------------------------------- search

    /// Runs `search` as user 1.
    async fn run_search(query: SearchQuery, committed: bool, limit: Option<u64>, media_type: Option<MediaType>, db: &DbConn, tmdb: &MockTmdb) -> Result<SearchResults, SrvErr> {
        search(query, committed, page(limit), media_type, &fixtures::user(), db, tmdb).await
    }

    const MOVIE_603_QUERY: &str = " : tmdb_id = 603 and type = \"movie\"";
    const SERIES_70_QUERY: &str = " : tmdb_id = 70 and type = \"series\"";

    #[tokio::test]
    async fn search_returns_internal_results_for_committed_query() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([vec![owned_movie_row(4, 603, "Alien"), owned_movie_row(6, 604, "Aliens")]])
            .append_query_results([Vec::<image_sizes::Model>::new()])
            .into_connection();

        // The page is filled by internal results, so TMDB is not asked (the mock has no expectations).
        let result = run_search(search_query("alien"), true, Some(2), None, &db, &MockTmdb::new()).await.ok().expect("search should succeed");

        assert!(result.query_valid);
        assert_eq!(result.indexes.iter().map(|i| i.id).collect::<Vec<_>>(), vec![4, 6]);
        assert_eq!(result.indexes[0].title, "Alien");
        assert!(result.external.is_empty());
        assert_statement_count(&test_support::db::log(db), 2);
    }

    #[tokio::test]
    async fn search_includes_tmdb_results_when_not_committed() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_search_multi().withf(|query| query == "matrix").times(1).returning(|_| Ok(multi_results(vec![
            multi_movie(603, "The Matrix"),
            multi_movie(604, "The Matrix Reloaded"),
            MultiResult::Person(MultiPersonResult { id: 6384, name: Some("Keanu Reeves".into()), ..Default::default() }),
            multi_tv(70, "Matrix: The Series"),
        ])));
        let db = test_support::db::mock()
            .append_query_results([vec![owned_movie_row(4, 603, "The Matrix")]])
            .append_query_results([Vec::<image_sizes::Model>::new()])
            .into_connection();

        let result = run_search(search_query("matrix"), false, Some(10), None, &db, &tmdb).await.ok().expect("search should succeed");

        assert!(result.query_valid);
        assert_eq!(result.indexes.len(), 1);
        // Movie 603 is already owned, and people are not media.
        assert_eq!(result.external.len(), 2);
        assert_eq!(result.external[0].external_id, 604);
        assert!(result.external[0].r#type == MediaType::Movie);
        assert_eq!(result.external[0].title, "The Matrix Reloaded");
        let poster = result.external[0].poster.as_ref().expect("external movie has a poster");
        assert_eq!(poster.path, "/poster-604.jpg");
        assert_eq!(poster.sizes.len(), 7);
        assert_eq!(result.external[1].external_id, 70);
        assert!(result.external[1].r#type == MediaType::Series);
        assert_eq!(result.external[1].title, "Matrix: The Series");
    }

    #[tokio::test]
    async fn search_limits_external_results_to_the_rest_of_the_page_when_page_not_full() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_search_multi().times(1).returning(|_| Ok(multi_results(vec![
            multi_movie(1, "One"), multi_movie(2, "Two"), multi_movie(3, "Three"), multi_movie(4, "Four"),
        ])));
        let db = test_support::db::mock()
            .append_query_results([vec![owned_movie_row(4, 603, "Matrix")]])
            .append_query_results([Vec::<image_sizes::Model>::new()])
            .into_connection();

        let result = run_search(search_query("matrix"), false, Some(3), None, &db, &tmdb).await.ok().expect("search should succeed");

        assert_eq!(result.external.iter().map(|e| e.external_id).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[tokio::test]
    async fn search_returns_only_requested_media_type_when_media_type_given() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_search_multi().times(1).returning(|_| Ok(multi_results(vec![multi_movie(603, "The Matrix"), multi_tv(70, "Matrix: The Series")])));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([Vec::<image_sizes::Model>::new()])
            .into_connection();

        let result = run_search(search_query("matrix"), true, None, Some(MediaType::Series), &db, &tmdb).await.ok().expect("search should succeed");

        assert_eq!(result.external.len(), 1);
        assert_eq!(result.external[0].external_id, 70);
        assert!(result.external[0].r#type == MediaType::Series);
    }

    #[tokio::test]
    async fn search_queries_tmdb_when_committed_and_name_short() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_search_multi().withf(|query| query == "al").times(1).returning(|_| Ok(multi_results(vec![multi_movie(1, "Al")])));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([Vec::<image_sizes::Model>::new()])
            .into_connection();

        let result = run_search(search_query("al"), true, None, None, &db, &tmdb).await.ok().expect("search should succeed");

        assert_eq!(result.external.len(), 1);
    }

    #[tokio::test]
    async fn search_skips_tmdb_when_not_committed_and_name_short() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([vec![owned_movie_row(4, 603, "Alien")]])
            .append_query_results([Vec::<image_sizes::Model>::new()])
            .into_connection();

        let result = run_search(search_query("ali"), false, None, None, &db, &MockTmdb::new()).await.ok().expect("search should succeed");

        assert_eq!(result.indexes.len(), 1);
        assert!(result.external.is_empty());
        assert_statement_count(&test_support::db::log(db), 2);
    }

    #[tokio::test]
    async fn search_skips_tmdb_when_query_is_not_a_plain_name_search() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([Vec::<image_sizes::Model>::new()])
            .into_connection();
        let query = SearchQuery { only_watched: true, ..search_query("matrix") };

        let result = run_search(query, true, None, None, &db, &MockTmdb::new()).await.ok().expect("search should succeed");

        assert!(result.query_valid);
        assert!(result.external.is_empty());
    }

    #[tokio::test]
    async fn search_looks_up_movie_on_tmdb_when_query_is_an_external_movie_id() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_details().withf(|id| *id == 603).times(1).returning(|id| Ok(test_support::tmdb::movie_details(id)));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([Vec::<image_sizes::Model>::new()])
            .into_connection();

        let result = run_search(search_query(MOVIE_603_QUERY), false, None, None, &db, &tmdb).await.ok().expect("search should succeed");

        assert_eq!(result.external.len(), 1);
        assert_eq!(result.external[0].external_id, 603);
        assert!(result.external[0].r#type == MediaType::Movie);
        assert_eq!(result.external[0].title, "Movie 603");
    }

    #[tokio::test]
    async fn search_looks_up_series_on_tmdb_when_query_is_an_external_series_id() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_series_details().withf(|id| *id == 70).times(1).returning(|id| Ok(test_support::tmdb::series_details(id)));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([Vec::<image_sizes::Model>::new()])
            .into_connection();

        let result = run_search(search_query(SERIES_70_QUERY), false, None, None, &db, &tmdb).await.ok().expect("search should succeed");

        assert_eq!(result.external.len(), 1);
        assert_eq!(result.external[0].external_id, 70);
        assert!(result.external[0].r#type == MediaType::Series);
        assert_eq!(result.external[0].title, "Series 70");
    }

    #[tokio::test]
    async fn search_skips_tmdb_when_external_id_already_owned() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([vec![owned_movie_row(4, 603, "The Matrix")]])
            .append_query_results([Vec::<image_sizes::Model>::new()])
            .into_connection();

        let result = run_search(search_query(MOVIE_603_QUERY), false, None, None, &db, &MockTmdb::new()).await.ok().expect("search should succeed");

        assert_eq!(result.indexes.len(), 1);
        assert!(result.external.is_empty());
    }

    #[tokio::test]
    async fn search_returns_no_external_results_when_external_id_not_found_on_tmdb() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_details().times(1).returning(|_| Err(error_with_status(StatusCode::NOT_FOUND)));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([Vec::<image_sizes::Model>::new()])
            .into_connection();

        let result = run_search(search_query(MOVIE_603_QUERY), false, None, None, &db, &tmdb).await.ok().expect("search should succeed");

        assert!(result.query_valid);
        assert!(result.external.is_empty());
    }

    #[tokio::test]
    async fn search_returns_err_when_tmdb_lookup_fails() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_details().times(1).returning(|_| Err(error_with_status(StatusCode::INTERNAL_SERVER_ERROR)));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([Vec::<image_sizes::Model>::new()])
            .into_connection();

        let result = run_search(search_query(MOVIE_603_QUERY), false, None, None, &db, &tmdb).await;

        assert!(matches!(result, Err(SrvErr::Integration(_))));
    }

    #[tokio::test]
    async fn search_returns_err_when_tmdb_search_fails() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_search_multi().times(1).returning(|_| Err(error("connection reset")));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([Vec::<image_sizes::Model>::new()])
            .into_connection();

        let result = run_search(search_query("matrix"), true, None, None, &db, &tmdb).await;

        assert!(matches!(result, Err(SrvErr::Integration(_))));
    }

    #[tokio::test]
    async fn search_returns_invalid_query_result_when_query_unparseable() {
        test_support::init();
        let db = test_support::db::mock().into_connection();

        // The part after " : " is an expression; "@" is not a valid token.
        let result = run_search(search_query("matrix : @"), true, None, None, &db, &MockTmdb::new()).await.ok().expect("search should succeed");

        assert!(!result.query_valid);
        assert!(result.indexes.is_empty());
        assert!(result.external.is_empty());
        assert_statement_count(&test_support::db::log(db), 0);
    }

    // ----------------------------------------------------------------- search_library / search_tmdb

    #[tokio::test]
    async fn search_library_returns_transpile_messages_when_query_is_invalid() {
        test_support::init();
        let db = test_support::db::mock().into_connection();

        let result = search_library(search_query(" : foo = 1"), page(None), None, &fixtures::user(), &db).await.ok().expect("search should succeed");

        let Err(messages) = result else { panic!("query should be invalid") };
        assert_eq!(messages, vec!["Construction: Unknown target 'foo'".to_string()]);
        assert_statement_count(&test_support::db::log(db), 0);
    }

    fn hit_ids(hits: &[TmdbHit]) -> Vec<(MediaType, i32)> {
        hits.iter().map(|h| (h.media_type(), h.tmdb_id())).collect()
    }

    #[tokio::test]
    async fn search_tmdb_keeps_only_movies_in_tmdb_order_when_type_is_movie() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_search_multi().withf(|query| query == "matrix").times(1).returning(|_| Ok(multi_results(vec![
            multi_tv(1, "One"),
            multi_movie(2, "Two"),
            MultiResult::Person(MultiPersonResult { id: 3, ..Default::default() }),
            multi_movie(4, "Four"),
        ])));

        let hits = search_tmdb("matrix".into(), Some(MediaType::Movie), &tmdb).await.ok().expect("search should succeed");

        assert!(hit_ids(&hits) == vec![(MediaType::Movie, 2), (MediaType::Movie, 4)]);
    }

    #[tokio::test]
    async fn search_tmdb_drops_people_and_collections_when_no_type() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_search_multi().times(1).returning(|_| Ok(multi_results(vec![
            multi_tv(1, "One"),
            MultiResult::Person(MultiPersonResult { id: 3, ..Default::default() }),
            MultiResult::Collection(Default::default()),
            multi_movie(4, "Four"),
        ])));

        let hits = search_tmdb("matrix".into(), None, &tmdb).await.ok().expect("search should succeed");

        assert!(hit_ids(&hits) == vec![(MediaType::Series, 1), (MediaType::Movie, 4)]);
    }

    // ----------------------------------------------------------------- on_watchlist

    #[tokio::test]
    async fn on_watchlist_true_when_media_on_watchlist() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::count_row(1)]])
            .append_query_results([[fixtures::count_row(1)]])
            .into_connection();

        let result = on_watchlist(10, MediaType::Movie, &fixtures::user(), &db).await;

        assert!(matches!(result, Ok(true)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_wrote(&log, "SELECT", &[Value::from(10)]);
    }

    #[tokio::test]
    async fn on_watchlist_false_when_media_not_on_watchlist() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::count_row(1)]])
            .append_query_results([[fixtures::count_row(0)]])
            .into_connection();

        let result = on_watchlist(10, MediaType::Movie, &fixtures::user(), &db).await;

        assert!(matches!(result, Ok(false)));
    }

    #[tokio::test]
    async fn on_watchlist_returns_not_found_when_media_missing() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::count_row(0)]])
            .into_connection();

        let result = on_watchlist(10, MediaType::Movie, &fixtures::user(), &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(10), Value::from(1)]);
    }

    // -------------------------------------------------------------------- backdrops

    #[tokio::test]
    async fn backdrops_returns_tmdb_candidates_with_backdrop_sizes() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_images().withf(|id, language| *id == 603 && language.as_deref() == Some("en"))
            .times(1).returning(|_, _| Ok(two_of_each_tmdb_images()));
        let db = test_support::db::mock()
            .append_query_results([[tmdb_movie()]])
            .append_query_results([[fixtures::language_model("en", "English")]])
            .append_query_results([Vec::<images::Model>::new()])
            .into_connection();

        let result = backdrops(10, MediaType::Movie, &fixtures::user(), &db, &tmdb).await.ok().expect("backdrops should succeed");

        // Largest first
        assert_eq!(result.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), vec!["/backdrop-2.jpg", "/backdrop-1.jpg"]);
        assert_eq!(result[0].language, None);
        assert_eq!(result[1].language.as_deref(), Some("English"));
        assert_eq!((result[0].original_width, result[0].original_height), (3840, 2160));
        assert!(result.iter().all(|c| !c.current && matches!(c.source, views::images::ImageSource::TMDB)));
        let slugs: Vec<&str> = result[0].sizes.iter().map(|s| s.slug.as_str()).collect();
        assert_eq!(slugs, vec!["tmdb/w300", "tmdb/w780", "tmdb/w1280", "tmdb/original"]);
    }

    #[tokio::test]
    async fn backdrops_marks_current_and_adds_manual_candidates_and_skips_current_tmdb_candidate() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_images().times(1).returning(|_, _| Ok(two_of_each_tmdb_images()));
        let current = images::Model { r#type: ImageType::Backdrop, source_path: Some("/backdrop-1.jpg".into()), ..fixtures::image_model(7) };
        let manual = images::Model { r#type: ImageType::Backdrop, source: ImageSource::Manual, source_path: None, ..fixtures::image_model(8) };
        let db = test_support::db::mock()
            .append_query_results([[media::Model { backdrop_image_id: Some(7), ..tmdb_movie() }]])
            .append_query_results([[fixtures::language_model("en", "English")]])
            .append_query_results([[(current, Some(fixtures::image_size_model(1, 7, 1920, 1080)))]])
            .append_query_results([[(manual, Some(fixtures::image_size_model(2, 8, 800, 450)))]])
            .into_connection();

        let result = backdrops(10, MediaType::Movie, &fixtures::user(), &db, &tmdb).await.ok().expect("backdrops should succeed");

        // The current backdrop first, then by size; the TMDB candidate for the current backdrop is dropped.
        // Records current behaviour: internal candidate paths have no leading slash, unlike index posters.
        assert_eq!(result.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), vec!["image-7.jpg", "/backdrop-2.jpg", "image-8.jpg"]);
        assert_eq!(result.iter().map(|c| c.current).collect::<Vec<_>>(), vec![true, false, false]);
        assert!(matches!(result[0].source, views::images::ImageSource::Internal));
        assert!(matches!(result[1].source, views::images::ImageSource::TMDB));
        assert!(matches!(result[2].source, views::images::ImageSource::Internal));
        assert_eq!((result[0].original_width, result[0].original_height), (1920, 1080));
    }

    #[tokio::test]
    async fn backdrops_uses_series_images_when_media_is_series() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_series_images().withf(|id, _| *id == 70).times(1).returning(|_, _| Ok(tmdb_images()));
        let db = test_support::db::mock()
            .append_query_results([[tmdb_series()]])
            .append_query_results([Vec::<languages::Model>::new()])
            .append_query_results([Vec::<images::Model>::new()])
            .into_connection();

        let result = backdrops(10, MediaType::Series, &fixtures::user(), &db, &tmdb).await.ok().expect("backdrops should succeed");

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "/backdrop-1.jpg");
        assert_eq!(result[0].language, None);
    }

    #[tokio::test]
    async fn backdrops_returns_empty_when_media_has_no_tmdb_id() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .into_connection();

        let result = backdrops(10, MediaType::Movie, &fixtures::user(), &db, &MockTmdb::new()).await.ok().expect("backdrops should succeed");

        assert!(result.is_empty());
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn backdrops_returns_not_found_when_media_missing() {
        test_support::init();
        let db = test_support::db::mock().append_query_results([Vec::<media::Model>::new()]).into_connection();

        let result = backdrops(10, MediaType::Movie, &fixtures::user(), &db, &MockTmdb::new()).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn backdrops_returns_err_when_tmdb_fails() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_images().times(1).returning(|_, _| Err(error("connection reset")));
        let db = test_support::db::mock().append_query_results([[tmdb_movie()]]).into_connection();

        let result = backdrops(10, MediaType::Movie, &fixtures::user(), &db, &tmdb).await;

        assert!(matches!(result, Err(SrvErr::Integration(_))));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    // -------------------------------------------------------------- update_backdrop

    #[tokio::test]
    async fn update_backdrop_pulls_image_and_deletes_unused_previous() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_image().withf(|path| path == "/new-backdrop.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        let before = media::Model { backdrop_image_id: Some(7), ..tmdb_movie() };
        let after = media::Model { backdrop_image_id: Some(20), ..tmdb_movie() };
        let db = test_support::db::mock().append_query_results([[before]]);
        let db = with_saved_tmdb_image(db, 20, ImageType::Backdrop)
            .append_query_results([[after]]);
        let db = with_deleted_unused_image(db, 7).into_connection();
        let store = InMemoryImageStore::new();
        store.insert("image-7", "300x450.jpg", b"previous".to_vec());
        let jobs = RecordingJobs::new();
        let update = BackdropUpdate { path: "/new-backdrop.jpg".into(), source: views::images::ImageSource::TMDB };

        let result = update_backdrop(10, MediaType::Movie, update, &fixtures::user(), &db, &tmdb, &store, &jobs, &SeqRandom::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from(20), Value::from(10)]);
        assert_wrote(&log, r#"DELETE FROM "image_sizes""#, &[Value::from(7)]);
        assert_wrote(&log, r#"DELETE FROM "images""#, &[Value::from(7)]);
        // The new image's directory is the only one left.
        assert_eq!(store.files(), vec![(Uuid::from_u128(1000).simple().to_string(), "300x450.png".to_string())]);
        assert_eq!(sizes(&jobs.take()), vec![(20, 100, 150)]);
    }

    #[tokio::test]
    async fn update_backdrop_keeps_previous_image_when_still_used() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_image().times(1).returning(|_| Ok(png_bytes(300, 450)));
        let before = media::Model { backdrop_image_id: Some(7), ..tmdb_movie() };
        let db = test_support::db::mock().append_query_results([[before.clone()]]);
        let db = with_saved_tmdb_image(db, 20, ImageType::Backdrop)
            .append_query_results([[media::Model { backdrop_image_id: Some(20), ..before }]])
            // image 7 is still the backdrop of another media
            .append_query_results([[fixtures::count_row(1)]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([[fixtures::count_row(0)]])
            .into_connection();
        let store = InMemoryImageStore::new();
        store.insert("image-7", "300x450.jpg", b"previous".to_vec());
        let update = BackdropUpdate { path: "/new-backdrop.jpg".into(), source: views::images::ImageSource::TMDB };

        let result = update_backdrop(10, MediaType::Movie, update, &fixtures::user(), &db, &tmdb, &store, &RecordingJobs::new(), &SeqRandom::new()).await;

        assert!(result.is_ok());
        assert_no_statement_starting_with(&test_support::db::log(db), "DELETE");
        assert!(store.get("image-7", "300x450.jpg").is_some());
    }

    #[tokio::test]
    async fn update_backdrop_sets_existing_image_when_source_internal() {
        test_support::init();
        let existing = images::Model { r#type: ImageType::Backdrop, ..fixtures::image_model(9) };
        let db = test_support::db::mock()
            .append_query_results([[tmdb_movie()]])
            .append_query_results([[existing]])
            .append_query_results([[media::Model { backdrop_image_id: Some(9), ..tmdb_movie() }]])
            .into_connection();
        let (tmdb, store, jobs) = unused_deps();
        let update = BackdropUpdate { path: "image-9.jpg".into(), source: views::images::ImageSource::Internal };

        let result = update_backdrop(10, MediaType::Movie, update, &fixtures::user(), &db, &tmdb, &store, &jobs, &SeqRandom::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 3);
        assert_wrote(&log, "SELECT", &[Value::from("image-9")]);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from(9), Value::from(10)]);
    }

    #[tokio::test]
    async fn update_backdrop_returns_not_found_when_media_missing() {
        test_support::init();
        let db = test_support::db::mock().append_query_results([Vec::<media::Model>::new()]).into_connection();
        let (tmdb, store, jobs) = unused_deps();
        let update = BackdropUpdate { path: "/new-backdrop.jpg".into(), source: views::images::ImageSource::TMDB };

        let result = update_backdrop(10, MediaType::Movie, update, &fixtures::user(), &db, &tmdb, &store, &jobs, &SeqRandom::new()).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    // ----------------------------------------------------------------------- posters

    #[tokio::test]
    async fn posters_returns_tmdb_candidates_with_poster_sizes() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_images().withf(|id, language| *id == 603 && language.as_deref() == Some("en"))
            .times(1).returning(|_, _| Ok(two_of_each_tmdb_images()));
        let db = test_support::db::mock()
            .append_query_results([[tmdb_movie()]])
            .append_query_results([[fixtures::language_model("en", "English")]])
            .append_query_results([Vec::<images::Model>::new()])
            .into_connection();

        let result = posters(10, MediaType::Movie, &fixtures::user(), &db, &tmdb).await.ok().expect("posters should succeed");

        // Largest first
        assert_eq!(result.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), vec!["/poster-2.jpg", "/poster-1.jpg"]);
        assert_eq!(result[0].language, None);
        assert_eq!(result[1].language.as_deref(), Some("English"));
        assert_eq!((result[0].original_width, result[0].original_height), (2000, 3000));
        assert!(result.iter().all(|c| !c.current && matches!(c.source, views::images::ImageSource::TMDB)));
        let slugs: Vec<&str> = result[0].sizes.iter().map(|s| s.slug.as_str()).collect();
        assert_eq!(slugs, vec!["tmdb/w92", "tmdb/w154", "tmdb/w185", "tmdb/w342", "tmdb/w500", "tmdb/w780", "tmdb/original"]);
    }

    #[tokio::test]
    async fn posters_marks_current_and_adds_manual_candidates_and_skips_current_tmdb_candidate() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_series_images().times(1).returning(|_, _| Ok(two_of_each_tmdb_images()));
        let current = images::Model { source_path: Some("/poster-1.jpg".into()), ..fixtures::image_model(7) };
        let manual = images::Model { source: ImageSource::Manual, source_path: None, ..fixtures::image_model(8) };
        let db = test_support::db::mock()
            .append_query_results([[media::Model { poster_image_id: Some(7), ..tmdb_series() }]])
            .append_query_results([[fixtures::language_model("en", "English")]])
            .append_query_results([[(current, Some(fixtures::image_size_model(1, 7, 1000, 1500)))]])
            .append_query_results([[(manual, Some(fixtures::image_size_model(2, 8, 100, 150)))]])
            .into_connection();

        let result = posters(10, MediaType::Series, &fixtures::user(), &db, &tmdb).await.ok().expect("posters should succeed");

        // Records current behaviour: internal candidate paths have no leading slash, unlike index posters.
        assert_eq!(result.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), vec!["image-7.jpg", "/poster-2.jpg", "image-8.jpg"]);
        assert_eq!(result.iter().map(|c| c.current).collect::<Vec<_>>(), vec![true, false, false]);
    }

    #[tokio::test]
    async fn posters_returns_empty_when_media_has_no_tmdb_id() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .into_connection();

        let result = posters(10, MediaType::Movie, &fixtures::user(), &db, &MockTmdb::new()).await.ok().expect("posters should succeed");

        assert!(result.is_empty());
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn posters_returns_not_found_when_media_missing() {
        test_support::init();
        let db = test_support::db::mock().append_query_results([Vec::<media::Model>::new()]).into_connection();

        let result = posters(10, MediaType::Movie, &fixtures::user(), &db, &MockTmdb::new()).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    // ---------------------------------------------------------------- update_poster

    #[tokio::test]
    async fn update_poster_pulls_image_and_deletes_unused_previous() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_image().withf(|path| path == "/new-poster.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        let before = media::Model { poster_image_id: Some(7), ..tmdb_movie() };
        let after = media::Model { poster_image_id: Some(20), ..tmdb_movie() };
        let db = test_support::db::mock().append_query_results([[before]]);
        let db = with_saved_tmdb_image(db, 20, ImageType::Poster)
            .append_query_results([[after]]);
        let db = with_deleted_unused_image(db, 7).into_connection();
        let store = InMemoryImageStore::new();
        store.insert("image-7", "300x450.jpg", b"previous".to_vec());
        let jobs = RecordingJobs::new();
        let update = PosterUpdate { path: "/new-poster.jpg".into(), source: views::images::ImageSource::TMDB };

        let result = update_poster(10, MediaType::Movie, update, &fixtures::user(), &db, &tmdb, &store, &jobs, &SeqRandom::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from(20), Value::from(10)]);
        assert_wrote(&log, r#"DELETE FROM "images""#, &[Value::from(7)]);
        assert_eq!(store.files(), vec![(Uuid::from_u128(1000).simple().to_string(), "300x450.png".to_string())]);
        assert_eq!(sizes(&jobs.take()), vec![(20, 100, 150)]);
    }

    #[tokio::test]
    async fn update_poster_sets_poster_without_deleting_when_media_had_none() {
        test_support::init();
        let existing = fixtures::image_model(9);
        let db = test_support::db::mock()
            .append_query_results([[tmdb_movie()]])
            .append_query_results([[existing]])
            .append_query_results([[media::Model { poster_image_id: Some(9), ..tmdb_movie() }]])
            .into_connection();
        let (tmdb, store, jobs) = unused_deps();
        let update = PosterUpdate { path: "image-9.jpg".into(), source: views::images::ImageSource::Internal };

        let result = update_poster(10, MediaType::Movie, update, &fixtures::user(), &db, &tmdb, &store, &jobs, &SeqRandom::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 3);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from(9), Value::from(10)]);
    }

    #[tokio::test]
    async fn update_poster_returns_bad_request_when_internal_image_unknown() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[tmdb_movie()]])
            .append_query_results([Vec::<images::Model>::new()])
            .into_connection();
        let (tmdb, store, jobs) = unused_deps();
        let update = PosterUpdate { path: "image-9.jpg".into(), source: views::images::ImageSource::Internal };

        let result = update_poster(10, MediaType::Movie, update, &fixtures::user(), &db, &tmdb, &store, &jobs, &SeqRandom::new()).await;

        assert!(matches!(result, Err(SrvErr::BadRequest(message)) if message == "Invalid path"));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_no_statement_starting_with(&log, "UPDATE");
    }

    // ------------------------------------------------------------------------ images

    #[tokio::test]
    async fn images_returns_both() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_images().withf(|id, language| *id == 603 && language.as_deref() == Some("en"))
            .times(1).returning(|_, _| Ok(two_of_each_tmdb_images()));
        let db = test_support::db::mock()
            .append_query_results([[tmdb_movie()]])
            .append_query_results([[fixtures::language_model("en", "English")]])
            .into_connection();

        let result = images(10, MediaType::Movie, &fixtures::user(), &db, &tmdb).await.ok().expect("images should succeed");

        assert_eq!(result.backdrops.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), vec!["/backdrop-2.jpg", "/backdrop-1.jpg"]);
        assert_eq!(result.posters.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), vec!["/poster-2.jpg", "/poster-1.jpg"]);
        assert_eq!(result.backdrops[1].language.as_deref(), Some("English"));
        assert_eq!(result.posters[1].language.as_deref(), Some("English"));
        assert_eq!(result.backdrops[0].sizes.len(), 4);
        assert_eq!(result.posters[0].sizes.len(), 7);
        assert!(result.backdrops.iter().chain(result.posters.iter()).all(|c| !c.current));
        assert_statement_count(&test_support::db::log(db), 2);
    }

    #[tokio::test]
    async fn images_uses_series_images_when_media_is_series() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_series_images().withf(|id, _| *id == 70).times(1).returning(|_, _| Ok(tmdb_images()));
        let db = test_support::db::mock()
            .append_query_results([[tmdb_series()]])
            .append_query_results([Vec::<languages::Model>::new()])
            .into_connection();

        let result = images(10, MediaType::Series, &fixtures::user(), &db, &tmdb).await.ok().expect("images should succeed");

        assert_eq!(result.backdrops.len(), 1);
        assert_eq!(result.posters.len(), 1);
    }

    #[tokio::test]
    async fn images_returns_empty_when_media_has_no_tmdb_id() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .into_connection();

        let result = images(10, MediaType::Movie, &fixtures::user(), &db, &MockTmdb::new()).await.ok().expect("images should succeed");

        assert!(result.backdrops.is_empty());
        assert!(result.posters.is_empty());
        assert_statement_count(&test_support::db::log(db), 1);
    }

    // ------------------------------------------------------------------ update_images

    #[tokio::test]
    async fn update_images_sets_both() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_image().withf(|path| path == "/new-backdrop.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        tmdb.expect_image().withf(|path| path == "/new-poster.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        let db = test_support::db::mock().append_query_results([[tmdb_movie()]]);
        let db = with_saved_tmdb_image(db, 20, ImageType::Backdrop);
        let db = with_saved_tmdb_image(db, 21, ImageType::Poster)
            .append_query_results([[media::Model { backdrop_image_id: Some(20), poster_image_id: Some(21), ..tmdb_movie() }]])
            .into_connection();
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();
        let update = ImagesUpdate {
            backdrop_path: Some("/new-backdrop.jpg".into()),
            poster_path: Some("/new-poster.jpg".into()),
            backdrop_source: Some(views::images::ImageSource::TMDB),
            poster_source: Some(views::images::ImageSource::TMDB),
        };

        let result = update_images(10, MediaType::Movie, update, &fixtures::user(), &db, &tmdb, &store, &jobs, &SeqRandom::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from(20), Value::from(21), Value::from(10)]);
        assert_eq!(store.files().len(), 2);
        assert_eq!(sizes(&jobs.take()), vec![(20, 100, 150), (21, 100, 150)]);
    }

    #[tokio::test]
    #[ignore = "known bug: update_images never deletes the previous images"]
    async fn update_images_deletes_unused_previous_images() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_image().withf(|path| path == "/new-backdrop.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        tmdb.expect_image().withf(|path| path == "/new-poster.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        let before = media::Model { backdrop_image_id: Some(7), poster_image_id: Some(8), ..tmdb_movie() };
        let after = media::Model { backdrop_image_id: Some(20), poster_image_id: Some(21), ..tmdb_movie() };
        let db = test_support::db::mock().append_query_results([[before]]);
        let db = with_saved_tmdb_image(db, 20, ImageType::Backdrop);
        let db = with_saved_tmdb_image(db, 21, ImageType::Poster)
            .append_query_results([[after]]);
        let db = with_deleted_unused_image(db, 7);
        let db = with_deleted_unused_image(db, 8).into_connection();
        let store = InMemoryImageStore::new();
        store.insert("image-7", "300x450.jpg", b"previous backdrop".to_vec());
        store.insert("image-8", "300x450.jpg", b"previous poster".to_vec());
        let update = ImagesUpdate {
            backdrop_path: Some("/new-backdrop.jpg".into()),
            poster_path: Some("/new-poster.jpg".into()),
            backdrop_source: Some(views::images::ImageSource::TMDB),
            poster_source: Some(views::images::ImageSource::TMDB),
        };

        let result = update_images(10, MediaType::Movie, update, &fixtures::user(), &db, &tmdb, &store, &RecordingJobs::new(), &SeqRandom::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "images""#, &[Value::from(7)]);
        assert_wrote(&log, r#"DELETE FROM "images""#, &[Value::from(8)]);
        // Only the two new images' directories are left.
        assert_eq!(store.files().len(), 2);
        assert!(store.get("image-7", "300x450.jpg").is_none());
        assert!(store.get("image-8", "300x450.jpg").is_none());
    }

    #[tokio::test]
    async fn update_images_sets_only_the_given_image() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_image().withf(|path| path == "/new-poster.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        let db = test_support::db::mock().append_query_results([[tmdb_movie()]]);
        let db = with_saved_tmdb_image(db, 21, ImageType::Poster)
            .append_query_results([[media::Model { poster_image_id: Some(21), ..tmdb_movie() }]])
            .into_connection();
        let update = ImagesUpdate {
            backdrop_path: None,
            poster_path: Some("/new-poster.jpg".into()),
            backdrop_source: None,
            poster_source: Some(views::images::ImageSource::TMDB),
        };

        let result = update_images(10, MediaType::Movie, update, &fixtures::user(), &db, &tmdb, &InMemoryImageStore::new(), &RecordingJobs::new(), &SeqRandom::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from(21), Value::from(10)]);
        let update = log.iter().find(|s| s.sql.starts_with(r#"UPDATE "media""#)).expect("media update");
        assert!(!update.sql.contains("\"backdrop_image_id\" ="), "the backdrop must not be written");
    }

    #[tokio::test]
    async fn update_images_returns_bad_request_when_source_missing() {
        test_support::init();
        let db = test_support::db::mock().append_query_results([[tmdb_movie()]]).into_connection();
        let (tmdb, store, jobs) = unused_deps();
        let update = ImagesUpdate { backdrop_path: Some("/new-backdrop.jpg".into()), poster_path: None, backdrop_source: None, poster_source: None };

        let result = update_images(10, MediaType::Movie, update, &fixtures::user(), &db, &tmdb, &store, &jobs, &SeqRandom::new()).await;

        assert!(matches!(result, Err(SrvErr::BadRequest(message)) if message == "Source not provided"));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn update_images_returns_not_found_when_media_missing() {
        test_support::init();
        let db = test_support::db::mock().append_query_results([Vec::<media::Model>::new()]).into_connection();
        let (tmdb, store, jobs) = unused_deps();
        let update = ImagesUpdate { backdrop_path: None, poster_path: None, backdrop_source: None, poster_source: None };

        let result = update_images(10, MediaType::Movie, update, &fixtures::user(), &db, &tmdb, &store, &jobs, &SeqRandom::new()).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    // ------------------------------------------------------------- delete_w_source

    #[tokio::test]
    async fn delete_w_source_deletes_media_and_orphaned_tags_and_genres_when_called_by_user() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[tmdb_movie()]])
            .append_exec_results([deleted(), deleted(), deleted()])
            .into_connection();

        // The source name is not consulted: a user deleting "with a source" deletes the whole media.
        let result = delete_w_source(603, "Torrent A".into(), MediaType::Movie, &fixtures::user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, "SELECT", &[Value::from(603), Value::from(1)]);
        assert_wrote(&log, r#"DELETE FROM "media""#, &[Value::from(10)]);
        assert_wrote(&log, r#"DELETE FROM "tags""#, &[]);
        assert_wrote(&log, r#"DELETE FROM "genres""#, &[]);
        assert_wrote(&log, "COMMIT", &[]);
    }

    #[tokio::test]
    async fn delete_w_source_deletes_media_when_bot_and_media_bot_controllable() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[media::Model { bot_controllable: true, ..tmdb_movie() }]])
            .append_exec_results([deleted(), deleted(), deleted()])
            .into_connection();

        let result = delete_w_source(603, "Torrent A".into(), MediaType::Movie, &fixtures::bot_user(), &db).await;

        assert!(result.is_ok());
        assert_wrote(&test_support::db::log(db), r#"DELETE FROM "media""#, &[Value::from(10)]);
    }

    #[tokio::test]
    async fn delete_w_source_deletes_only_source_when_bot_and_media_not_bot_controllable() {
        test_support::init();
        let source = entities::sources::Model { bot_controllable: true, ..fixtures::source_model(3, 10, "Torrent A") };
        let db = test_support::db::mock()
            .append_query_results([[tmdb_movie()]])
            .append_query_results([[source]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_exec_results([deleted()])
            .into_connection();

        let result = delete_w_source(603, "Torrent A".into(), MediaType::Movie, &fixtures::bot_user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "sources""#, &[Value::from(3)]);
        assert_no_statement_starting_with(&log, r#"DELETE FROM "media""#);
    }

    #[tokio::test]
    async fn delete_w_source_returns_not_found_when_media_missing() {
        test_support::init();
        let db = test_support::db::mock().append_query_results([Vec::<media::Model>::new()]).into_connection();

        let result = delete_w_source(603, "Torrent A".into(), MediaType::Movie, &fixtures::user(), &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn delete_w_source_returns_not_found_when_bot_and_source_missing() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[tmdb_movie()]])
            .append_query_results([Vec::<entities::sources::Model>::new()])
            .into_connection();

        let result = delete_w_source(603, "Torrent A".into(), MediaType::Movie, &fixtures::bot_user(), &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 2);
    }

    #[tokio::test]
    async fn delete_w_source_returns_unauthorized_when_bot_and_source_not_bot_controllable() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[tmdb_movie()]])
            .append_query_results([[fixtures::source_model(3, 10, "Torrent A")]])
            .into_connection();

        let result = delete_w_source(603, "Torrent A".into(), MediaType::Movie, &fixtures::bot_user(), &db).await;

        assert!(matches!(result, Err(SrvErr::Unauthorized)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_no_statement_starting_with(&log, "DELETE");
    }

    // ---------------------------------------------------------------------- delete

    #[tokio::test]
    async fn delete_removes_media_and_unused_images() {
        test_support::init();
        let media = media::Model { poster_image_id: Some(7), backdrop_image_id: Some(8), ..tmdb_movie() };
        let db = test_support::db::mock()
            .append_query_results([[media]])
            // the media, then the orphaned tags and genres
            .append_exec_results([deleted(), deleted(), deleted()]);
        let db = with_deleted_unused_image(db, 7);
        let db = with_deleted_unused_image(db, 8)
            // manual image references of the media, whose image is deleted too
            .append_query_results([[fixtures::manual_image_reference_model(9, 10)]]);
        let db = with_deleted_unused_image(db, 9).into_connection();
        let store = InMemoryImageStore::new();
        for directory in ["image-7", "image-8", "image-9", "image-other"] {
            store.insert(directory, "300x450.jpg", b"image".to_vec());
        }

        let result = delete(10, MediaType::Movie, &fixtures::user(), &db, &store).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "media""#, &[Value::from(10)]);
        assert_wrote(&log, r#"DELETE FROM "manual_image_references""#, &[Value::from(10)]);
        for image_id in [7, 8, 9] {
            assert_wrote(&log, r#"DELETE FROM "images""#, &[Value::from(image_id)]);
        }
        assert_wrote(&log, "COMMIT", &[]);
        assert_eq!(store.files(), vec![("image-other".to_string(), "300x450.jpg".to_string())]);
    }

    #[tokio::test]
    async fn delete_keeps_image_when_still_used() {
        test_support::init();
        let media = media::Model { poster_image_id: Some(7), ..tmdb_movie() };
        let db = test_support::db::mock()
            .append_query_results([[media]])
            .append_exec_results([deleted(), deleted(), deleted()])
            // image 7 is also used by another media
            .append_query_results([[fixtures::count_row(1)]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([Vec::<manual_image_references::Model>::new()])
            .into_connection();
        let store = InMemoryImageStore::new();
        store.insert("image-7", "300x450.jpg", b"image".to_vec());

        let result = delete(10, MediaType::Movie, &fixtures::user(), &db, &store).await;

        assert!(result.is_ok());
        assert_no_statement_starting_with(&test_support::db::log(db), r#"DELETE FROM "images""#);
        assert_eq!(store.files().len(), 1);
    }

    #[tokio::test]
    async fn delete_deletes_media_when_bot_and_media_bot_controllable() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[media::Model { bot_controllable: true, ..tmdb_movie() }]])
            .append_exec_results([deleted(), deleted(), deleted()])
            .append_query_results([Vec::<manual_image_references::Model>::new()])
            .into_connection();

        let result = delete(10, MediaType::Movie, &fixtures::bot_user(), &db, &InMemoryImageStore::new()).await;

        assert!(result.is_ok());
        assert_wrote(&test_support::db::log(db), r#"DELETE FROM "media""#, &[Value::from(10)]);
    }

    #[tokio::test]
    async fn delete_returns_unauthorized_when_bot_and_media_not_bot_controllable() {
        test_support::init();
        let db = test_support::db::mock().append_query_results([[tmdb_movie()]]).into_connection();

        let result = delete(10, MediaType::Movie, &fixtures::bot_user(), &db, &InMemoryImageStore::new()).await;

        assert!(matches!(result, Err(SrvErr::Unauthorized)));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn delete_returns_not_found_when_media_missing() {
        test_support::init();
        let db = test_support::db::mock().append_query_results([Vec::<media::Model>::new()]).into_connection();

        let result = delete(10, MediaType::Movie, &fixtures::user(), &db, &InMemoryImageStore::new()).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    // --------------------------------------------------------------- apply_changes

    #[tokio::test]
    async fn apply_changes_updates_unlocked_media_properties() {
        test_support::init();
        let changes = with_language(property_changes(&[
            ("overview", serde_json::json!("A new overview")),
            ("homepage", serde_json::json!("https://new.example.test")),
            ("imdb_id", serde_json::json!("tt7654321")),
            ("iso_639_1", serde_json::json!("fr")),
            ("origin_country", serde_json::json!(["FR", "BE", 7])),
        ]), "en");
        let db = test_support::db::mock()
            .append_query_results([Vec::<media_locks::Model>::new()])
            .append_query_results([[tmdb_movie()]])
            .into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_movie(), &changes, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_wrote(&log, r#"UPDATE "media""#, &[
            Value::from("A new overview"), Value::from("https://new.example.test"), Value::from("tt7654321"),
            Value::from("fr"), Value::from(vec!["FR".to_string(), "BE".to_string()]), Value::from(10),
        ]);
    }

    #[tokio::test]
    async fn apply_changes_skips_locked_media_properties() {
        test_support::init();
        let changes = with_language(property_changes(&[
            ("overview", serde_json::json!("A new overview")),
            ("homepage", serde_json::json!("https://new.example.test")),
            ("imdb_id", serde_json::json!("tt7654321")),
            ("iso_639_1", serde_json::json!("fr")),
            ("origin_country", serde_json::json!(["FR"])),
        ]), "en");
        let locks = media_locks::Model {
            overview: true, homepage: true, imdb_id: true, original_language: true, origin_country: true,
            ..fixtures::media_locks_model(10)
        };
        let db = test_support::db::mock().append_query_results([[locks]]).into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_movie(), &changes, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_no_statement_starting_with(&log, "UPDATE");
    }

    #[tokio::test]
    async fn apply_changes_writes_only_unlocked_properties_when_some_locked() {
        test_support::init();
        let changes = property_changes(&[
            ("homepage", serde_json::json!("https://new.example.test")),
            ("imdb_id", serde_json::json!("tt7654321")),
        ]);
        let locks = media_locks::Model { homepage: true, ..fixtures::media_locks_model(10) };
        let db = test_support::db::mock()
            .append_query_results([[locks]])
            .append_query_results([[tmdb_movie()]])
            .into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_movie(), &changes, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from("tt7654321")]);
        let update = log.iter().find(|s| s.sql.starts_with(r#"UPDATE "media""#)).expect("media update");
        assert!(!update.values.as_ref().expect("update values").0.contains(&Value::from("https://new.example.test")), "the locked homepage must not be written");
    }

    #[tokio::test]
    async fn apply_changes_ignores_overview_when_language_not_english() {
        test_support::init();
        let changes = with_language(property_changes(&[("overview", serde_json::json!("Ein neuer Text"))]), "de");
        let db = test_support::db::mock().append_query_results([Vec::<media_locks::Model>::new()]).into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_movie(), &changes, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_no_statement_starting_with(&log, "UPDATE");
    }

    #[tokio::test]
    async fn apply_changes_ignores_changes_without_a_value() {
        test_support::init();
        let mut changes = property_changes(&[
            ("homepage", serde_json::json!("https://new.example.test")),
            ("imdb_id", serde_json::json!("tt7654321")),
        ]);
        changes.changes[0].items[0].value = None;
        changes.changes[1].items.clear();
        let db = test_support::db::mock().append_query_results([Vec::<media_locks::Model>::new()]).into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_movie(), &changes, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_no_statement_starting_with(&log, "UPDATE");
    }

    #[tokio::test]
    async fn apply_changes_ignores_unknown_keys() {
        test_support::init();
        let changes = property_changes(&[("adult", serde_json::json!(true)), ("runtime", serde_json::json!(90))]);
        let db = test_support::db::mock().append_query_results([Vec::<media_locks::Model>::new()]).into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_movie(), &changes, &db).await;

        assert!(result.is_ok());
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn apply_changes_sets_poster_and_backdrop_when_media_has_none() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_image().withf(|path| path == "/new-poster.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        tmdb.expect_image().withf(|path| path == "/new-backdrop.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        let db = test_support::db::mock().append_query_results([Vec::<media_locks::Model>::new()]);
        let db = with_saved_tmdb_image(db, 21, ImageType::Poster);
        let db = with_saved_tmdb_image(db, 22, ImageType::Backdrop)
            .append_query_results([[media::Model { poster_image_id: Some(21), backdrop_image_id: Some(22), ..tmdb_movie() }]])
            .into_connection();
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();
        let changes = images_change(Some("/new-poster.jpg"), Some("/new-backdrop.jpg"));

        let result = run_apply_changes(&tmdb_movie(), &changes, &db, &tmdb, &store, &jobs).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from(21), Value::from(22), Value::from(10)]);
        assert_eq!(store.files().len(), 2);
        assert_eq!(sizes(&jobs.take()), vec![(21, 100, 150), (22, 100, 150)]);
    }

    #[tokio::test]
    async fn apply_changes_keeps_images_when_media_already_has_them() {
        test_support::init();
        let media = media::Model { poster_image_id: Some(1), backdrop_image_id: Some(2), ..tmdb_movie() };
        let db = test_support::db::mock().append_query_results([Vec::<media_locks::Model>::new()]).into_connection();
        let changes = images_change(Some("/new-poster.jpg"), Some("/new-backdrop.jpg"));
        let (tmdb, store, jobs) = unused_deps();

        let result = run_apply_changes(&media, &changes, &db, &tmdb, &store, &jobs).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_no_statement_starting_with(&log, "UPDATE");
        assert!(store.files().is_empty());
        assert!(jobs.take().is_empty());
    }

    #[tokio::test]
    async fn apply_changes_skips_locked_images() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_image().withf(|path| path == "/new-backdrop.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        let locks = media_locks::Model { poster_path: true, ..fixtures::media_locks_model(10) };
        let db = test_support::db::mock().append_query_results([[locks]]);
        let db = with_saved_tmdb_image(db, 22, ImageType::Backdrop)
            .append_query_results([[media::Model { backdrop_image_id: Some(22), ..tmdb_movie() }]])
            .into_connection();
        let changes = images_change(Some("/new-poster.jpg"), Some("/new-backdrop.jpg"));

        let result = run_apply_changes(&tmdb_movie(), &changes, &db, &tmdb, &InMemoryImageStore::new(), &RecordingJobs::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from(22), Value::from(10)]);
        let update = log.iter().find(|s| s.sql.starts_with(r#"UPDATE "media""#)).expect("media update");
        assert!(!update.sql.contains("\"poster_image_id\" ="), "the locked poster must not be written");
    }

    #[tokio::test]
    async fn apply_changes_skips_images_when_change_has_no_file_path() {
        test_support::init();
        let mut changes = images_change(Some("/unused.jpg"), None);
        changes.changes[0].items[0].value = Some(serde_json::json!({"poster": {"iso_639_1": "en"}}));
        let db = test_support::db::mock().append_query_results([Vec::<media_locks::Model>::new()]).into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_movie(), &changes, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
    }

    #[tokio::test]
    async fn apply_changes_adds_alternative_title_when_title_changed_in_english() {
        test_support::init();
        let changes = with_language(property_changes(&[("title", serde_json::json!("The Matrix"))]), "en");
        let db = test_support::db::mock()
            .append_query_results([Vec::<media_locks::Model>::new()])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([[fixtures::title_model(5, 10, "The Matrix", false)]])
            .into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_movie(), &changes, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        // Never the primary title, and no write to the media row.
        assert_wrote(&log, r#"INSERT INTO "titles""#, &[Value::from(10), Value::from("The Matrix"), Value::from(false)]);
        assert_no_statement_starting_with(&log, r#"UPDATE "media""#);
    }

    #[tokio::test]
    async fn apply_changes_adds_title_when_language_is_the_original_language() {
        test_support::init();
        let media = media::Model { original_language: Some("ja".into()), ..tmdb_movie() };
        let changes = with_language(property_changes(&[("original_title", serde_json::json!("Matorikkusu"))]), "ja");
        let db = test_support::db::mock()
            .append_query_results([Vec::<media_locks::Model>::new()])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([[fixtures::title_model(5, 10, "Matorikkusu", false)]])
            .into_connection();

        let result = run_apply_changes_unused_deps(&media, &changes, &db).await;

        assert!(result.is_ok());
        assert_wrote(&test_support::db::log(db), r#"INSERT INTO "titles""#, &[Value::from(10), Value::from("Matorikkusu"), Value::from(false)]);
    }

    #[tokio::test]
    async fn apply_changes_skips_title_when_language_neither_english_nor_original() {
        test_support::init();
        let media = media::Model { original_language: Some("ja".into()), ..tmdb_movie() };
        let changes = with_language(property_changes(&[("name", serde_json::json!("Die Matrix"))]), "de");
        let db = test_support::db::mock().append_query_results([Vec::<media_locks::Model>::new()]).into_connection();

        let result = run_apply_changes_unused_deps(&media, &changes, &db).await;

        assert!(result.is_ok());
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn apply_changes_skips_title_when_media_already_has_it() {
        test_support::init();
        let changes = with_language(property_changes(&[("original_name", serde_json::json!("The Matrix"))]), "en");
        let db = test_support::db::mock()
            .append_query_results([Vec::<media_locks::Model>::new()])
            .append_query_results([[fixtures::count_row(1)]])
            .into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_movie(), &changes, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_no_statement_starting_with(&log, "INSERT");
    }

    #[tokio::test]
    async fn apply_changes_adds_alternative_title_from_alternative_titles_change() {
        test_support::init();
        let changes = with_language(property_changes(&[
            ("alternative_titles", serde_json::json!({"iso_3166_1": "US", "title": "Matrix, The", "type": ""})),
        ]), "en");
        let db = test_support::db::mock()
            .append_query_results([Vec::<media_locks::Model>::new()])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([[fixtures::title_model(5, 10, "Matrix, The", false)]])
            .into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_movie(), &changes, &db).await;

        assert!(result.is_ok());
        assert_wrote(&test_support::db::log(db), r#"INSERT INTO "titles""#, &[Value::from(10), Value::from("Matrix, The"), Value::from(false)]);
    }

    #[tokio::test]
    async fn apply_changes_adds_genre_to_media_when_genre_changed() {
        test_support::init();
        let changes = property_changes(&[("genres", serde_json::json!({"id": 878, "name": "science fiction"}))]);
        let db = test_support::db::mock()
            .append_query_results([Vec::<media_locks::Model>::new()])
            // genre not on the media yet, not in the genres table yet; then the new genre and its link
            .append_query_results([Vec::<genres::Model>::new()])
            .append_query_results([Vec::<genres::Model>::new()])
            .append_query_results([[fixtures::genre_model(5, "Science Fiction", sea_orm_active_enums::MediaType::Movie)]])
            .append_query_results([[fixtures::media_genre_model(10, 5)]])
            .into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_movie(), &changes, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "genres""#, &[Value::from("Science Fiction")]);
        assert_wrote(&log, r#"INSERT INTO "media_genres""#, &[Value::from(10), Value::from(5)]);
    }

    // ------------------------------------------------------------------- is_locked

    #[test]
    fn is_locked_false_when_no_locks_row() {
        let locks: Option<media_locks::Model> = None;

        assert!(!is_locked(&locks, |l| l.overview));
    }

    #[test]
    fn is_locked_returns_the_property_value_when_locks_row_exists() {
        let locks = Some(media_locks::Model { overview: true, ..fixtures::media_locks_model(10) });

        assert!(is_locked(&locks, |l| l.overview));
        assert!(!is_locked(&locks, |l| l.homepage));
    }

    // ------------------------------------------------------------ build_media_index

    #[test]
    fn build_media_index_uses_unset_title_when_none() {
        test_support::init();
        let media = fixtures::movie_media_model(10, 1);

        let index = build_media_index((media, None, None), None);

        assert_eq!(index.id, 10);
        assert_eq!(index.title, "Unknown");
        assert!(index.poster.is_none());
    }

    #[test]
    fn build_media_index_maps_title_stars_type_and_poster_with_sizes() {
        test_support::init();
        let media = media::Model { stars: Some(3.5), ..fixtures::series_media_model(10, 1) };
        let poster = images::Model { file_type: ImageFileType::WebP, ..fixtures::image_model(2) };
        let size = fixtures::image_size_model(1, 2, 300, 450);
        let sizes = vec![&size];

        let index = build_media_index((media, Some(poster), Some(fixtures::title_model(1, 10, "Andor", true))), Some(&sizes));

        assert_eq!(index.title, "Andor");
        assert_eq!(index.stars, Some(3.5));
        assert!(index.r#type == MediaType::Series);
        let poster = index.poster.expect("media has a poster");
        assert_eq!(poster.path, "/image-2.webp");
        assert_eq!(poster.sizes.len(), 1);
        assert_eq!((poster.sizes[0].slug.as_str(), poster.sizes[0].width, poster.sizes[0].height), ("300x450", 300, 450));
    }

    #[test]
    fn build_media_index_has_no_sizes_when_none_given() {
        test_support::init();
        let media = media::Model { poster_image_id: Some(2), ..fixtures::movie_media_model(10, 1) };

        let index = build_media_index((media, Some(fixtures::image_model(2)), None), None);

        assert!(index.poster.expect("media has a poster").sizes.is_empty());
    }

    #[test]
    fn build_media_indexes_gives_each_media_the_sizes_of_its_own_poster() {
        test_support::init();
        let first = media::Model { poster_image_id: Some(2), ..fixtures::movie_media_model(1, 1) };
        let second = media::Model { poster_image_id: Some(3), ..fixtures::movie_media_model(2, 1) };
        let third = fixtures::movie_media_model(3, 1);
        let data = vec![
            (first, Some(fixtures::image_model(2)), Some(fixtures::title_model(1, 1, "First", true))),
            (second, Some(fixtures::image_model(3)), Some(fixtures::title_model(2, 2, "Second", true))),
            (third, None, Some(fixtures::title_model(3, 3, "Third", true))),
        ];
        let poster_sizes = vec![
            fixtures::image_size_model(1, 2, 300, 450),
            fixtures::image_size_model(2, 3, 100, 150),
            fixtures::image_size_model(3, 2, 100, 150),
            fixtures::image_size_model(4, 9, 500, 750),
        ];

        let indexes = build_media_indexes(data, poster_sizes);

        assert_eq!(indexes.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), vec!["First", "Second", "Third"]);
        let first_sizes: Vec<&str> = indexes[0].poster.as_ref().expect("poster").sizes.iter().map(|s| s.slug.as_str()).collect();
        assert_eq!(first_sizes, vec!["300x450", "100x150"]);
        let second_sizes: Vec<&str> = indexes[1].poster.as_ref().expect("poster").sizes.iter().map(|s| s.slug.as_str()).collect();
        assert_eq!(second_sizes, vec!["100x150"]);
        assert!(indexes[2].poster.is_none());
    }

    #[test]
    fn build_media_indexes_returns_empty_when_no_data() {
        test_support::init();

        assert!(build_media_indexes(Vec::new(), Vec::new()).is_empty());
    }

    // ---------------------------------------------------------- try_set_media_lock

    #[tokio::test]
    async fn try_set_media_lock_returns_false_when_property_is_not_a_media_lock() {
        test_support::init();
        let db = test_support::db::mock().into_connection();

        let result = try_set_media_lock(tmdb_movie(), "runtime", true, &db).await;

        assert!(matches!(result, Ok(false)));
        assert_statement_count(&test_support::db::log(db), 0);
    }

    #[tokio::test]
    async fn try_set_media_lock_updates_existing_locks_row() {
        test_support::init();
        let existing = fixtures::media_locks_model(10);
        let db = test_support::db::mock()
            .append_query_results([[existing.clone()]])
            .append_query_results([[media_locks::Model { overview: true, ..existing }]])
            .into_connection();

        let result = try_set_media_lock(tmdb_movie(), "overview", true, &db).await;

        assert!(matches!(result, Ok(true)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_wrote(&log, r#"UPDATE "media_locks""#, &[Value::from(true), Value::from(10)]);
    }

    #[tokio::test]
    async fn try_set_media_lock_clears_lock_when_unlocking() {
        test_support::init();
        let existing = media_locks::Model { homepage: true, ..fixtures::media_locks_model(10) };
        let db = test_support::db::mock()
            .append_query_results([[existing.clone()]])
            .append_query_results([[media_locks::Model { homepage: false, ..existing }]])
            .into_connection();

        let result = try_set_media_lock(tmdb_movie(), "homepage", false, &db).await;

        assert!(matches!(result, Ok(true)));
        assert_wrote(&test_support::db::log(db), r#"UPDATE "media_locks""#, &[Value::from(false), Value::from(10)]);
    }

    #[tokio::test]
    async fn try_set_media_lock_inserts_locks_row_when_media_has_none() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media_locks::Model>::new()])
            .append_query_results([[media_locks::Model { poster_path: true, ..fixtures::media_locks_model(10) }]])
            .into_connection();

        let result = try_set_media_lock(tmdb_movie(), "poster_path", true, &db).await;

        assert!(matches!(result, Ok(true)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_wrote(&log, r#"INSERT INTO "media_locks""#, &[Value::from(10), Value::from(true)]);
    }

    // ----------------------------------------------------------------- fetch_media

    #[tokio::test]
    async fn fetch_media_returns_media_when_owned_by_user_and_of_type() {
        test_support::init();
        let db = test_support::db::mock().append_query_results([[tmdb_movie()]]).into_connection();

        let result = fetch_media(10, MediaType::Movie, &fixtures::user(), &db).await;

        assert_eq!(result.ok().expect("media should be found").id, 10);
        assert_wrote(&test_support::db::log(db), "SELECT", &[Value::from(10), Value::from(1)]);
    }

    #[tokio::test]
    async fn fetch_media_returns_not_found_when_media_belongs_to_another_user() {
        test_support::init();
        // The database finds nothing for user 2: the query filters on the user.
        let db = test_support::db::mock().append_query_results([Vec::<media::Model>::new()]).into_connection();
        let other_user = CurrentUser::from(fixtures::user_model(2));

        let result = fetch_media(10, MediaType::Movie, &other_user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_wrote(&test_support::db::log(db), "SELECT", &[Value::from(10), Value::from(2)]);
    }

    // ---------------------------------------------------- fetch_id_or_pull_image

    #[tokio::test]
    async fn fetch_id_or_pull_image_returns_id_of_existing_image_when_source_internal() {
        test_support::init();
        let db = test_support::db::mock().append_query_results([[fixtures::image_model(9)]]).into_connection();
        let (tmdb, store, jobs) = unused_deps();

        let result = fetch_id_or_pull_image(views::images::ImageSource::Internal, "image-9.jpg", ImageType::Poster, &db, &tmdb, &store, &jobs, &SeqRandom::new()).await;

        assert!(matches!(result, Ok(9)));
        assert_wrote(&test_support::db::log(db), "SELECT", &[Value::from("image-9")]);
    }

    #[tokio::test]
    async fn fetch_id_or_pull_image_returns_bad_request_when_internal_path_has_no_known_extension() {
        test_support::init();
        let (tmdb, store, jobs) = unused_deps();
        for path in ["image-9", "image-9.gif"] {
            let db = test_support::db::mock().into_connection();

            let result = fetch_id_or_pull_image(views::images::ImageSource::Internal, path, ImageType::Poster, &db, &tmdb, &store, &jobs, &SeqRandom::new()).await;

            assert!(matches!(result, Err(SrvErr::BadRequest(ref message)) if message == "Invalid path"), "path {path:?}");
            assert_statement_count(&test_support::db::log(db), 0);
        }
    }

    #[tokio::test]
    async fn fetch_id_or_pull_image_pulls_image_from_tmdb_when_source_tmdb() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_image().withf(|path| path == "/new.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        let db = with_saved_tmdb_image(MockDatabase::new(sea_orm::DatabaseBackend::Postgres), 20, ImageType::Poster).into_connection();
        let jobs = RecordingJobs::new();

        let result = fetch_id_or_pull_image(views::images::ImageSource::TMDB, "/new.jpg", ImageType::Poster, &db, &tmdb, &InMemoryImageStore::new(), &jobs, &SeqRandom::new()).await;

        assert!(matches!(result, Ok(20)));
        assert_eq!(sizes(&jobs.take()), vec![(20, 100, 150)]);
    }
}
