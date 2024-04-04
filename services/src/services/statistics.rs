use futures::try_join;
use sea_orm::{ColumnTrait, DbConn, EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};
use sea_orm::prelude::Expr;
use sea_orm::sea_query::Func;

use entities::{genres, languages, logs, media, titles};
use entities::prelude::{Genres, Languages, Logs, Media, Titles};
use entities::sea_orm_active_enums::MediaType;
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
        .group_by(media::Column::Id).group_by(titles::Column::Id).order_by_desc(logs::Column::Id.count()).order_by_desc(media::Column::Stars).limit(1)
        .find_also_related(Titles).filter(titles::Column::Primary.eq(true)).one(db);
    let watched_series_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Series))
        .inner_join(Logs)
        .group_by(media::Column::Id).group_by(titles::Column::Id).order_by_desc(logs::Column::Id.count()).order_by_desc(media::Column::Stars).limit(1)
        .find_also_related(Titles).filter(titles::Column::Primary.eq(true)).one(db);

    let rated_movie_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Movie))
        .inner_join(Logs)
        .group_by(media::Column::Id).group_by(titles::Column::Id).order_by_desc(media::Column::Stars).order_by_desc(logs::Column::Id.count()).limit(1)
        .find_also_related(Titles).filter(titles::Column::Primary.eq(true)).one(db);
    let rated_series_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Series))
        .inner_join(Logs)
        .group_by(media::Column::Id).group_by(titles::Column::Id).order_by_desc(media::Column::Stars).order_by_desc(logs::Column::Id.count()).limit(1)
        .find_also_related(Titles).filter(titles::Column::Primary.eq(true)).one(db);

    let genres_future = async {
        let db_genres: Vec<ComparativeStats> = Genres::find().inner_join(Media)
            .select_only()
            .column(genres::Column::Name)
            .column_as(Expr::col((media::Entity, media::Column::Id)).count(), "count")
            .filter(media::Column::UserId.eq(user.id))
            .group_by(genres::Column::Id)
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
            movie: watched_movie.map(|m| {
                crate::services::media::build_media_index(m)
            }),
            series: watched_series.map(|s| {
                crate::services::media::build_media_index(s)
            }),
        },
        highest_rated: CategoryStats {
            movie: rated_movie.map(|m| {
                crate::services::media::build_media_index(m)
            }),
            series: rated_series.map(|m| {
                crate::services::media::build_media_index(m)
            }),
        },
        average_rating: AvgRatingStats {
            overall: media_avg.map_or(Some(0f64), |x| x.avg),
            movies: movies_avg.map_or(Some(0f64), |x| x.avg),
            series: series_avg.map_or(Some(0f64), |x| x.avg),
        },
    })
}