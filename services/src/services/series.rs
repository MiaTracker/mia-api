use std::env;
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, DbConn, EntityTrait, ModelTrait, NotSet, TransactionTrait};
use entities::{genres, media, media_genres, seasons, series, titles};
use views::infrastructure::traits::IntoActiveModel;
use views::tmdb::{Season, SeriesDetails};
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;
use sea_orm::{QueryFilter, ColumnTrait};
use entities::prelude::{Genres, Languages, Logs, Media, Series, Sources, Tags, Titles};
use entities::sea_orm_active_enums::MediaType;
use integrations::tmdb;
use views::languages::Language;
use views::logs::Log;
use views::media::MediaIndex;
use views::sources::Source;
use views::tags::Tag;
use views::titles::AlternativeTitle;

pub async fn create(tmdb_id: i32, user: &CurrentUser, db: &DbConn) -> Result<bool, SrvErr> {
    let med_res = media::Entity::find().filter(media::Column::TmdbId.eq(tmdb_id))
        .filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Series)).one(db).await;

    match med_res {
        Ok(media) => {
            if media.is_some() {
                return Ok(false)
            }
        }
        Err(err) => { return Err(SrvErr::from(err)); }
    }

    let trans = db.begin().await?;

    let tmdb_series = match tmdb::series::details(tmdb_id).await {
        Ok(series) => { series }
        Err(err) => { return Err(SrvErr::from(err)); }
    };

    let mut media = <&SeriesDetails as IntoActiveModel<media::ActiveModel>>::into_active_model(&tmdb_series);
    let mut series = <&SeriesDetails as IntoActiveModel<series::ActiveModel>>::into_active_model(&tmdb_series);

    media.user_id = Set(user.id);
    let inserted_media = media.insert(db).await?;
    series.id = Set(inserted_media.id);
    series.insert(db).await?;

    if let Some(seasons) = &tmdb_series.seasons {
        for season in seasons {
            let mut model = <&Season as IntoActiveModel<seasons::ActiveModel>>::into_active_model(&season);

            model.series_id = Set(inserted_media.id);
            model.insert(db).await?;
        }
    }

    for genre in &tmdb_series.genres {
        let existing = genres::Entity::find().filter(genres::Column::TmdbId.eq(genre.id)).one(db).await?;
        if existing.is_none() {
            return Err(SrvErr::MasterdataOutdated);
        }

        let media_genre = media_genres::ActiveModel {
            media_id: Set(inserted_media.id),
            genre_id: Set(existing.unwrap().id),
        };
        media_genre.insert(db).await?;
    }


    let model = titles::ActiveModel {
        id: NotSet,
        media_id: Set(inserted_media.id),
        primary: Set(true),
        title: Set(tmdb_series.name)
    };
    model.insert(db).await?;

    let titles = tmdb::series::alternative_titles(tmdb_id).await?;
    for title in titles.results {
        let model = titles::ActiveModel {
            id: NotSet,
            media_id: Set(inserted_media.id),
            primary: Set(false),
            title: Set(title.title)
        };
        model.insert(db).await?;
    }

    //TODO: implement episode fetching

    trans.commit().await?;
    Ok(true)
}

pub async fn index(user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let media = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Series)).all(db).await?;
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

pub async fn details(user: &CurrentUser, series_id: i32, db: &DbConn) -> Result<Option<views::series::SeriesDetails>, SrvErr> {
    let db_media = Media::find().filter(media::Column::Id.eq(series_id))
        .filter(media::Column::UserId.eq(user.id)).filter(media::Column::Type.eq(MediaType::Series)).one(db).await?;

    if db_media.is_none() {
        return Ok(None);
    }
    let db_media = db_media.unwrap();

    let db_series =  db_media.find_related(Series).one(db).await?;
    let db_series = db_series.expect("DB in invalid state!");

    let db_titles = db_media.find_related(Titles).all(db).await?;

    let language = if let Some(language_id) = db_media.original_language.clone() {
        Languages::find_by_id(language_id).one(db).await?.map(|l| {
            Language::from(l)
        })
    } else {
        None
    };

    let genres = db_media.find_related(Genres).all(db).await?.iter().map(|m| {
        views::genres::Genre::from(m)
    }).collect();
    let tags = db_media.find_related(Tags).all(db).await?.iter().map(|m| {
        Tag::from(m)
    }).collect();
    let logs = db_media.find_related(Logs).all(db).await?.iter().map(|m| {
        Log::from(m)
    }).collect();
    let sources = db_media.find_related(Sources).all(db).await?.iter().map(|m| {
        Source::from(m)
    }).collect();

    let mut title = None;
    let alternative_titles = db_titles.iter().filter_map(|t| {
        if t.primary {
            title = Some(t);
            None
        } else {
            Some(AlternativeTitle::from(t))
        }
    }).collect();
    let title = if let Some(t) = title {
        t.title.clone()
    } else {
        env::var("UNSET_MEDIA_TITLE").expect("UNSET_MEDIA_TITLE not set!")
    };

    let series = views::series::SeriesDetails {
        id: db_media.id,
        poster_path: db_media.poster_path,
        backdrop_path: db_media.backdrop_path,
        stars: db_media.stars,
        title,
        alternative_titles,
        first_air_date: db_series.first_air_date,
        number_of_episodes: db_series.number_of_episodes,
        number_of_seasons: db_series.number_of_seasons,
        status: db_series.status,
        r#type: db_series.r#type,
        overview: db_media.overview,
        tmdb_vote_average: db_media.tmdb_vote_average,
        original_language: language,
        genres,
        tags,
        sources,
        logs,
    };

    Ok(Some(series))
}