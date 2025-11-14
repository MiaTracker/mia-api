use futures::try_join;
use sea_orm::{ColumnTrait, DbConn, EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};
use sea_orm::prelude::Expr;
use sea_orm::sea_query::Func;

use entities::{genres, image_sizes, images, languages, logs, media, titles};
use entities::prelude::{Genres, ImageSizes, Languages, Logs, Media, Titles};
use entities::sea_orm_active_enums::MediaType;
use entities::traits::linked::MediaPosters;
use views::statistics::{AvgRatingStats, CategoryStats, ComparativeStats, LogStats, MediaStats, Stats};
use views::users::CurrentUser;

use crate::infrastructure::SrvErr;

pub async fn stats(user: &CurrentUser, db: &DbConn) -> Result<Stats, SrvErr> {
    let media_count_future = Media::find().filter(media::Column::UserId.eq(user.id)).count(db);
    let movie_count_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Movie)).count(db);
    let series_count_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Series)).count(db);

    #[derive(FromQueryResult)]
    struct AvgModel {
        pub avg: Option<f64>
    }

    let media_avg_future = Media::find()
        .select_only()
        .expr_as_(Func::avg(Expr::col(media::Column::Stars)), "avg")
        .filter(media::Column::UserId.eq(user.id))
        .into_model::<AvgModel>()
        .one(db);
    let movies_avg_future = Media::find()
        .select_only()
        .expr_as_(Func::avg(Expr::col(media::Column::Stars)), "avg")
        .filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Movie))
        .into_model::<AvgModel>()
        .one(db);
    let series_avg_future = Media::find()
        .select_only()
        .expr_as_(Func::avg(Expr::col(media::Column::Stars)), "avg")
        .filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Series))
        .into_model::<AvgModel>()
        .one(db);

    let log_count_future = Media::find().filter(media::Column::UserId.eq(user.id)).inner_join(Logs).count(db);
    let completed_log_count_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(logs::Column::Completed.eq(true)).inner_join(Logs).count(db);
    let uncompleted_log_count_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(logs::Column::Completed.eq(false)).inner_join(Logs).count(db);

    let watched_movie_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Movie))
        .inner_join(Logs)
        .find_also_linked(MediaPosters)
        .group_by(media::Column::Id).group_by(titles::Column::Id).order_by_desc(logs::Column::Id.count()).order_by_desc(media::Column::Stars.if_null(-1)).limit(1)
        .find_also_related(Titles).filter(titles::Column::Primary.eq(true)).one(db);
    let watched_series_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Series))
        .inner_join(Logs)
        .find_also_linked(MediaPosters)
        .group_by(media::Column::Id).group_by(titles::Column::Id).order_by_desc(logs::Column::Id.count()).order_by_desc(media::Column::Stars.if_null(-1)).limit(1)
        .find_also_related(Titles).filter(titles::Column::Primary.eq(true)).one(db);

    let rated_movie_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Movie))
        .inner_join(Logs)
        .find_also_linked(MediaPosters)
        .group_by(media::Column::Id).group_by(titles::Column::Id).order_by_desc(media::Column::Stars.if_null(-1)).order_by_desc(logs::Column::Id.count()).limit(1)
        .find_also_related(Titles).filter(titles::Column::Primary.eq(true)).one(db);
    let rated_series_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Series))
        .inner_join(Logs)
        .find_also_linked(MediaPosters)
        .group_by(media::Column::Id).group_by(titles::Column::Id).order_by_desc(media::Column::Stars.if_null(-1)).order_by_desc(logs::Column::Id.count()).limit(1)
        .find_also_related(Titles).filter(titles::Column::Primary.eq(true)).one(db);

    let genres_future = async {
        let db_genres: Vec<ComparativeStats> = Genres::find().inner_join(Media)
            .select_only()
            .column(genres::Column::Name)
            .column_as(Expr::col((media::Entity, media::Column::Id)).count(), "count")
            .filter(media::Column::UserId.eq(user.id))
            .group_by(genres::Column::Name)
            .order_by_desc(Expr::col((media::Entity, media::Column::Id)).count())
            .into_model::<ComparativeStats>()
            .all(db).await?;
        let other_sum = db_genres.iter().skip(6).map(|x| x.count).sum();
        let mut genres: Vec<ComparativeStats> = Vec::with_capacity(7);
        genres.extend(db_genres.into_iter().take(6));
        if other_sum > 0 {
            genres.push(
                ComparativeStats {
                    name: "Other".to_string(),
                    count: other_sum,
                }
            )
        }

        Ok(genres)
    };

    let languages_future = async {
        let db_genres: Vec<ComparativeStats> = Languages::find().inner_join(Media)
            .select_only()
            .column_as(languages::Column::EnglishName, "name")
            .column_as(Expr::col((media::Entity, media::Column::Id)).count(), "count")
            .filter(media::Column::UserId.eq(user.id))
            .group_by(languages::Column::Iso6391)
            .order_by_desc(Expr::col((media::Entity, media::Column::Id)).count())
            .into_model::<ComparativeStats>()
            .all(db).await?;
        let other_sum = db_genres.iter().skip(6).map(|x| x.count).sum();
        let mut languages: Vec<ComparativeStats> = Vec::with_capacity(7);
        languages.extend(db_genres.into_iter().take(6));
        if other_sum > 0 {
            languages.push(
                ComparativeStats {
                    name: "Other".to_string(),
                    count: other_sum,
                }
            )
        }

        Ok(languages)
    };


    let (media_count, movie_count, series_count,
        log_count, completed_log_count, uncompleted_log_count,
        watched_movie, watched_series, genres,
        languages, rated_movie, rated_series,
        media_avg, movies_avg, series_avg) =
        try_join!(media_count_future, movie_count_future, series_count_future,
            log_count_future, completed_log_count_future, uncompleted_log_count_future,
            watched_movie_future, watched_series_future, genres_future, languages_future,
            rated_movie_future, rated_series_future, media_avg_future, movies_avg_future, series_avg_future)?;

    let watched_movie_id = watched_movie.as_ref().and_then(|p| Some(p.0.id));
    let watched_series_id = watched_series.as_ref().and_then(|p| Some(p.0.id));
    let rated_movie_id = rated_movie.as_ref().and_then(|p| Some(p.0.id));
    let rated_series_id = rated_series.as_ref().and_then(|p| Some(p.0.id));

    let media_vec: Vec<(media::Model, Option<images::Model>, Option<titles::Model>)> =
        vec![watched_movie, watched_series, rated_movie, rated_series].into_iter()
        .filter_map(|p| p).collect();
    let poster_ids: Vec<i32> = media_vec.iter()
        .filter_map(|p| p.0.poster_image_id).collect();
    let poster_sizes = ImageSizes::find()
        .filter(image_sizes::Column::ImageId.is_in(poster_ids)).all(db).await?;

    let indexes = crate::services::media::build_media_indexes(media_vec, poster_sizes);

    let mut watched_movie_idx = Option::None;
    let mut watched_series_idx = Option::None;
    let mut rated_movie_idx = Option::None;
    let mut rated_series_idx = Option::None;

    for index in indexes {
        if watched_movie_id.is_some_and(|i| i == index.id) {
            watched_movie_idx = Some(index);
        }
        else if watched_series_id.is_some_and(|i| i == index.id) {
            watched_series_idx = Some(index);
        }
        else if rated_movie_id.is_some_and(|i| i == index.id) {
            rated_movie_idx = Some(index);
        }
        else if rated_series_id.is_some_and(|i| i == index.id) {
            rated_series_idx = Some(index);
        }
    }


    Ok(Stats {
        media: MediaStats {
            count: media_count,
            movies: movie_count,
            series: series_count,
        },
        logs: LogStats {
            logs: log_count,
            completed: completed_log_count,
            uncompleted: uncompleted_log_count,
        },
        genres,
        languages,
        most_watched: CategoryStats {
            movie: watched_movie_idx,
            series: watched_series_idx,
        },
        highest_rated: CategoryStats {
            movie: rated_movie_idx,
            series: rated_series_idx,
        },
        average_rating: AvgRatingStats {
            overall: media_avg.map_or(Some(0f64), |x| x.avg),
            movies: movies_avg.map_or(Some(0f64), |x| x.avg),
            series: series_avg.map_or(Some(0f64), |x| x.avg),
        },
    })
}