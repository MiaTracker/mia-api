use std::env;
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, DbConn, EntityTrait, ModelTrait, NotSet, TransactionTrait};
use entities::{genres, media, media_genres, media_tags, sea_orm_active_enums, seasons, series, titles, user_media};
use views::infrastructure::traits::IntoActiveModel;
use views::tmdb::{Genre, Season, SeriesDetails};
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;
use sea_orm::{QueryFilter, ColumnTrait};
use entities::prelude::{Genres, Languages, Logs, Media, MediaTags, Series, Sources, Tags, Titles, UserMedia};
use entities::sea_orm_active_enums::MediaType;
use integrations::tmdb;
use views::languages::Language;
use views::logs::Log;
use views::media::MediaIndex;
use views::sources::Source;
use views::tags::Tag;
use views::titles::AlternativeTitle;

pub async fn create(tmdb_id: i32, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let trans = db.begin().await?;

    let tmdb_series = match tmdb::series::details(tmdb_id).await {
        Ok(series) => { series }
        Err(err) => { return Err(SrvErr::from(err)); }
    };

    let med_res = media::Entity::find().filter(media::Column::TmdbId.eq(tmdb_id))
        .filter(media::Column::Type.eq(sea_orm_active_enums::MediaType::Series)).one(db).await;
    let med_opt = match med_res {
        Ok(media) => { media }
        Err(err) => { return Err(SrvErr::DB(err)); }
    };
    let mut media = <&SeriesDetails as IntoActiveModel<media::ActiveModel>>::into_active_model(&tmdb_series);
    let mut series = <&SeriesDetails as IntoActiveModel<series::ActiveModel>>::into_active_model(&tmdb_series);
    let inserted_media;
    if let Some(med) = med_opt {
        media.id = Set(med.id);
        media.date_added = NotSet;
        inserted_media = media.update(db).await?;
        series.id = Set(med.id);
        series.update(db).await?;
    } else {
        inserted_media = media.insert(db).await?;
        series.id = Set(inserted_media.id);
        series.insert(db).await?;
    }

    if let Some(seasons) = &tmdb_series.seasons {
        for season in seasons {
            let mut model = <&Season as IntoActiveModel<seasons::ActiveModel>>::into_active_model(&season);
            let existing = seasons::Entity::find().filter(seasons::Column::SeriesId.eq(inserted_media.id))
                .filter(seasons::Column::SeasonNumber.eq(season.season_number)).one(db).await?;
            if let Some(existing_season) = existing {
                model.id = Set(existing_season.id);
                model.update(db).await?;
            } else {
                model.series_id = Set(inserted_media.id);
                model.insert(db).await?;
            }
        }
    }

    let user_media = user_media::Entity::find()
        .filter(user_media::Column::UserId.eq(user.id))
        .filter(user_media::Column::MediaId.eq(inserted_media.id))
        .one(db).await?;
    if user_media.is_none() {
        let user_media_am = user_media::ActiveModel {
            media_id: Set(inserted_media.id),
            user_id: Set(user.id),
            stars: Set(None),
        };
        user_media_am.insert(db).await?;
    }

    for genre in &tmdb_series.genres {
        let mut model = <&Genre as IntoActiveModel<genres::ActiveModel>>::into_active_model(&genre);
        let existing = genres::Entity::find().filter(genres::Column::TmdbId.eq(genre.id)).one(db).await?;
        let inserted;
        if let Some(existing_genre) = existing {
            model.id = Set(existing_genre.id);
            inserted = model.update(db).await?;
        } else {
            inserted = model.insert(db).await?;
        }

        let media_genre = media_genres::Entity::find()
            .filter(media_genres::Column::MediaId.eq(inserted_media.id))
            .filter(media_genres::Column::GenreId.eq(inserted.id))
            .one(db).await?;
        if media_genre.is_none() {
            let media_genre = media_genres::ActiveModel {
                media_id: Set(inserted_media.id),
                genre_id: Set(inserted.id),
            };
            media_genre.insert(db).await?;
        }
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
    Ok(())
}

pub async fn index(user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let user_media = UserMedia::find().filter(user_media::Column::UserId.eq(user.id)).all(db).await?;
    let mut indexes = Vec::with_capacity(user_media.len());
    for um in user_media {
        let media = um.find_related(Media).one(db).await?;
        if media.is_none() {
            return Err(SrvErr::Internal("User media exists without a media reference!".to_string()));
        }
        let media = media.unwrap();
        if media.r#type != MediaType::Series {
            continue;
        }

        let title = media.find_related(Titles).filter(titles::Column::Primary.eq(true)).one(db).await?;
        let title = if let Some(title) = title {
            title.title
        } else { env::var("UNSET_MEDIA_TITLE").expect("UNSET_MEDIA_TITLE not set!") };

        let index = MediaIndex {
            id: media.id,
            r#type: views::media::MediaType::from(media.r#type),
            poster_path: media.poster_path,
            stars: um.stars,
            title,
        };
        indexes.push(index);
    }

    Ok(indexes)
}

pub async fn details(user: &CurrentUser, series_id: i32, db: &DbConn) -> Result<Option<views::series::SeriesDetails>, SrvErr> {
    let db_user_media = UserMedia::find().filter(user_media::Column::UserId.eq(user.id)).filter(user_media::Column::MediaId.eq(series_id)).one(db).await?;
    if db_user_media.is_none() {
        return Ok(None);
    }
    let db_user_media = db_user_media.unwrap();

    let db_media = Media::find_by_id(series_id).one(db).await?;
    let db_media = db_media.expect("DB in invalid state!");

    if db_media.r#type != MediaType::Series { return Ok(None); }

    let db_series =  Series::find_by_id(series_id).one(db).await?;
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
    let tags = MediaTags::find()
        .filter(media_tags::Column::MediaId.eq(db_media.id))
        .filter(media_tags::Column::UserId.eq(user.id))
        .find_with_related(Tags).all(db).await?.iter().flat_map(|(_, ms)| {
        ms.iter().map(|m| {
            Tag::from(m)
        })
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
        stars: db_user_media.stars,
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