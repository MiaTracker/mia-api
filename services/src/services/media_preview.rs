//! What movie and series previews share: the library check, the matching of TMDB genres and
//! languages to masterdata, and TMDB images. A preview reads TMDB and the masterdata, and writes
//! nothing.

use std::collections::HashMap;
use chrono::NaiveDate;
use entities::prelude::{Genres, Languages, Media};
use entities::sea_orm_active_enums::MediaType;
use entities::{genres, media};
use http::StatusCode;
use integrations::tmdb::views::Genre as TmdbGenre;
use sea_orm::{ColumnTrait, DbConn, EntityTrait, QueryFilter};
use views::genres::Genre;
use views::images::Image;
use views::languages::Language;
use views::users::CurrentUser;

use crate::infrastructure::constants::tmdb_config;
use crate::infrastructure::traits::IntoView;
use crate::infrastructure::SrvErr;

/// The answer to a preview request.
pub enum Preview<T> {
    /// The title is in the user's library already, under this media id. Nothing was fetched from TMDB.
    InLibrary(i32),
    /// The title is not in the library; this is what TMDB says about it.
    New(T),
}

/// The id of the user's media of `media_type` with TMDB id `tmdb_id`, if they have one. One query.
pub(crate) async fn library_id(tmdb_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<Option<i32>, SrvErr> {
    let found = Media::find().filter(media::Column::TmdbId.eq(tmdb_id))
        .filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(media_type)).one(db).await?;
    Ok(found.map(|m| m.id))
}

/// The masterdata genres of `media_type` for `tmdb_genres`, in TMDB's order, in one query (none
/// when there are no genres). Fails with `MasterdataOutdated` when a genre is not in the masterdata,
/// as creating the media would.
pub(crate) async fn load_genres(tmdb_genres: &[TmdbGenre], media_type: MediaType, db: &DbConn) -> Result<Vec<Genre>, SrvErr> {
    if tmdb_genres.is_empty() {
        return Ok(Vec::new());
    }
    let tmdb_ids: Vec<i32> = tmdb_genres.iter().map(|g| g.id).collect();
    let rows = Genres::find().filter(genres::Column::TmdbId.is_in(tmdb_ids))
        .filter(genres::Column::Type.eq(media_type)).all(db).await?;
    let by_tmdb_id: HashMap<i32, &genres::Model> = rows.iter()
        .filter_map(|g| g.tmdb_id.map(|tmdb_id| (tmdb_id, g))).collect();
    tmdb_genres.iter()
        .map(|g| by_tmdb_id.get(&g.id).map(|row| Genre::from(*row)).ok_or(SrvErr::MasterdataOutdated))
        .collect()
}

/// The masterdata language with ISO 639-1 code `code`, in one query. None, without a query, when
/// `code` is missing or empty; None when the masterdata does not have the language.
pub(crate) async fn load_language(code: Option<&str>, db: &DbConn) -> Result<Option<Language>, SrvErr> {
    match code {
        None | Some("") => Ok(None),
        Some(code) => Ok(Languages::find_by_id(code.to_string()).one(db).await?.map(Language::from)),
    }
}

/// The poster and the backdrop at TMDB paths `poster_path` and `backdrop_path`, offered in all
/// TMDB sizes (`tmdb/<size>` slugs, which `/img/tmdb/{slug}/{path}` serves).
pub(crate) fn tmdb_images(poster_path: &Option<String>, backdrop_path: &Option<String>) -> (Option<Image>, Option<Image>) {
    let config = tmdb_config();
    let poster = poster_path.as_ref().map(|path| Image {
        path: path.clone(),
        sizes: config.images.poster_sizes.iter().map(|s| s.into_view()).collect(),
    });
    let backdrop = backdrop_path.as_ref().map(|path| Image {
        path: path.clone(),
        sizes: config.images.backdrop_sizes.iter().map(|s| s.into_view()).collect(),
    });
    (poster, backdrop)
}

/// A TMDB failure as a service error: TMDB not knowing the title is `NotFound`.
pub(crate) fn tmdb_error(error: integrations::infrastructure::Error) -> SrvErr {
    if error.status_code == Some(StatusCode::NOT_FOUND) {
        SrvErr::NotFound
    } else {
        SrvErr::from(error)
    }
}

/// `text`, or None when it is missing or empty.
pub(crate) fn non_empty(text: Option<String>) -> Option<String> {
    text.filter(|t| !t.is_empty())
}

/// The date in `text` (`YYYY-MM-DD`), or None when it is missing, empty or malformed.
pub(crate) fn parse_date(text: Option<&str>) -> Option<NaiveDate> {
    text.and_then(|t| NaiveDate::parse_from_str(t, "%Y-%m-%d").ok())
}
