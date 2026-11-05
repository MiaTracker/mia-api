use futures::try_join;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::{Alias, Func, IntoColumnRef, SimpleExpr};
use sea_orm::{ColumnTrait, DbConn, EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};

use entities::prelude::{Genres, ImageSizes, Languages, Logs, Media, Titles};
use entities::sea_orm_active_enums::MediaType;
use entities::traits::linked::MediaPosters;
use entities::{genres, image_sizes, images, languages, logs, media, titles};
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
        .group_by(media::Column::Id).group_by(titles::Column::Id).group_by(SimpleExpr::Column((Alias::new("r0"), images::Column::Id).into_column_ref()))
        .order_by_desc(logs::Column::Id.count()).order_by_desc(media::Column::Stars.if_null(-1)).limit(1)
        .find_also_related(Titles).filter(titles::Column::Primary.eq(true)).one(db);
    let watched_series_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Series))
        .inner_join(Logs)
        .find_also_linked(MediaPosters)
        .group_by(media::Column::Id).group_by(titles::Column::Id).group_by(SimpleExpr::Column((Alias::new("r0"), images::Column::Id).into_column_ref()))
        .order_by_desc(logs::Column::Id.count()).order_by_desc(media::Column::Stars.if_null(-1)).limit(1)
        .find_also_related(Titles).filter(titles::Column::Primary.eq(true)).one(db);

    let rated_movie_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Movie))
        .inner_join(Logs)
        .find_also_linked(MediaPosters)
        .group_by(media::Column::Id).group_by(titles::Column::Id).group_by(SimpleExpr::Column((Alias::new("r0"), images::Column::Id).into_column_ref()))
        .order_by_desc(media::Column::Stars.if_null(-1)).order_by_desc(logs::Column::Id.count()).limit(1)
        .find_also_related(Titles).filter(titles::Column::Primary.eq(true)).one(db);
    let rated_series_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Series))
        .inner_join(Logs)
        .find_also_linked(MediaPosters)
        .group_by(media::Column::Id).group_by(titles::Column::Id).group_by(SimpleExpr::Column((Alias::new("r0"), images::Column::Id).into_column_ref()))
        .order_by_desc(media::Column::Stars.if_null(-1)).order_by_desc(logs::Column::Id.count()).limit(1)
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

    // One slot per entry. The same media can fill several of them (most watched and highest rated).
    let picks = [watched_movie, watched_series, rated_movie, rated_series];
    let filled = picks.each_ref().map(Option::is_some);

    let media_vec: Vec<(media::Model, Option<images::Model>, Option<titles::Model>)> =
        picks.into_iter().flatten().collect();
    let poster_ids: Vec<i32> = media_vec.iter()
        .filter_map(|p| p.0.poster_image_id).collect();
    let poster_sizes = ImageSizes::find()
        .filter(image_sizes::Column::ImageId.is_in(poster_ids)).all(db).await?;

    // One index per media, in order, so the filled slots take them in turn.
    let mut indexes = crate::services::media::build_media_indexes(media_vec, poster_sizes).into_iter();
    let [watched_movie_idx, watched_series_idx, rated_movie_idx, rated_series_idx] =
        filled.map(|filled| if filled { indexes.next() } else { None });


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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use sea_orm::{MockDatabase, Value};
    use super::*;
    use crate::test_support;
    use crate::test_support::db::assert_statement_count;
    use crate::test_support::fixtures;

    // ---------------------------------------------------------------- helpers

    /// Queues the six count queries of `stats`, in the order it polls them: media, movies, series,
    /// logs, completed logs, uncompleted logs.
    fn with_counts(db: MockDatabase, counts: [i64; 6]) -> MockDatabase {
        counts.into_iter().fold(db, |db, n| db.append_query_results([[fixtures::count_row(n)]]))
    }

    /// Queues the three average queries of `stats`, in poll order: overall, movies, series.
    fn with_averages(db: MockDatabase, averages: [Option<f64>; 3]) -> MockDatabase {
        averages.into_iter().fold(db, |db, avg| db.append_query_results([[fixtures::avg_row(avg)]]))
    }

    fn no_index_rows() -> Vec<BTreeMap<String, Value>> {
        Vec::new()
    }

    fn no_comparative_rows() -> Vec<BTreeMap<&'static str, Value>> {
        Vec::new()
    }

    /// A row of a "most watched" / "highest rated" query: a media with a poster and a primary title.
    fn index_row(media: media::Model, poster_id: i32, title: &str) -> BTreeMap<String, Value> {
        let media = media::Model { poster_image_id: Some(poster_id), ..media };
        let id = media.id;
        fixtures::media_index_row(media, Some(fixtures::image_model(poster_id)), Some(fixtures::title_model(id, id, title, true)))
    }

    // ---------------------------------------------------------------- stats

    #[tokio::test]
    async fn stats_aggregates_all_counters() {
        test_support::init();
        let watched_movie = media::Model { stars: Some(7.0), ..fixtures::movie_media_model(1, 1) };
        let watched_series = fixtures::series_media_model(2, 1);
        let rated_movie = media::Model { stars: Some(9.5), ..fixtures::movie_media_model(3, 1) };
        let rated_series = media::Model { stars: Some(8.0), ..fixtures::series_media_model(4, 1) };
        // The queries are polled in source order by try_join!: counts, watched, genres, languages, rated, averages.
        let db = test_support::db::mock();
        let db = with_counts(db, [10, 6, 4, 20, 15, 5])
            .append_query_results([[index_row(watched_movie, 11, "Alien")]])
            .append_query_results([[index_row(watched_series, 12, "Bluey")]])
            .append_query_results([[
                fixtures::comparative_row("Drama", 9), fixtures::comparative_row("Comedy", 8),
                fixtures::comparative_row("Action", 7), fixtures::comparative_row("Horror", 6),
                fixtures::comparative_row("Family", 5), fixtures::comparative_row("Crime", 4),
                fixtures::comparative_row("Western", 2), fixtures::comparative_row("War", 1),
            ]])
            .append_query_results([[fixtures::comparative_row("English", 7), fixtures::comparative_row("French", 3)]])
            .append_query_results([[index_row(rated_movie, 13, "Brazil")]])
            .append_query_results([[index_row(rated_series, 14, "Dark")]]);
        let db = with_averages(db, [Some(7.25), Some(7.0), Some(8.0)])
            .append_query_results([[
                fixtures::image_size_model(1, 11, 300, 450), fixtures::image_size_model(2, 11, 100, 150),
                fixtures::image_size_model(3, 14, 300, 450),
            ]])
            .into_connection();

        let stats = stats(&fixtures::user(), &db).await.expect("stats");

        assert_eq!((stats.media.count, stats.media.movies, stats.media.series), (10, 6, 4));
        assert_eq!((stats.logs.logs, stats.logs.completed, stats.logs.uncompleted), (20, 15, 5));
        let genres: Vec<(&str, i64)> = stats.genres.iter().map(|g| (g.name.as_str(), g.count)).collect();
        assert_eq!(genres, vec![
            ("Drama", 9), ("Comedy", 8), ("Action", 7), ("Horror", 6), ("Family", 5), ("Crime", 4), ("Other", 3),
        ]);
        let languages: Vec<(&str, i64)> = stats.languages.iter().map(|l| (l.name.as_str(), l.count)).collect();
        assert_eq!(languages, vec![("English", 7), ("French", 3)]);
        let watched_movie = stats.most_watched.movie.expect("most watched movie");
        assert_eq!((watched_movie.id, watched_movie.title.as_str(), watched_movie.stars), (1, "Alien", Some(7.0)));
        let poster = watched_movie.poster.expect("poster");
        let sizes: Vec<&str> = poster.sizes.iter().map(|s| s.slug.as_str()).collect();
        assert_eq!(sizes, vec!["300x450", "100x150"]);
        let watched_series = stats.most_watched.series.expect("most watched series");
        assert_eq!((watched_series.id, watched_series.title.as_str()), (2, "Bluey"));
        let rated_movie = stats.highest_rated.movie.expect("highest rated movie");
        assert_eq!((rated_movie.id, rated_movie.title.as_str(), rated_movie.stars), (3, "Brazil", Some(9.5)));
        let rated_series = stats.highest_rated.series.expect("highest rated series");
        assert_eq!((rated_series.id, rated_series.title.as_str()), (4, "Dark"));
        assert_eq!(rated_series.poster.expect("poster").sizes.len(), 1);
        assert_eq!(
            (stats.average_rating.overall, stats.average_rating.movies, stats.average_rating.series),
            (Some(7.25), Some(7.0), Some(8.0)),
        );
        // 6 counts, 4 media lookups, genres, languages, 3 averages, poster sizes.
        assert_statement_count(&test_support::db::log(db), 16);
    }

    #[tokio::test]
    async fn stats_returns_empty_stats_when_user_has_no_media() {
        test_support::init();
        let db = with_counts(test_support::db::mock(), [0; 6])
            .append_query_results([no_index_rows()])
            .append_query_results([no_index_rows()])
            .append_query_results([no_comparative_rows()])
            .append_query_results([no_comparative_rows()])
            .append_query_results([no_index_rows()])
            .append_query_results([no_index_rows()]);
        let db = with_averages(db, [None, None, None])
            .append_query_results([Vec::<entities::image_sizes::Model>::new()])
            .into_connection();

        let stats = stats(&fixtures::user(), &db).await.expect("stats");

        assert_eq!((stats.media.count, stats.media.movies, stats.media.series), (0, 0, 0));
        assert_eq!((stats.logs.logs, stats.logs.completed, stats.logs.uncompleted), (0, 0, 0));
        assert!(stats.genres.is_empty());
        assert!(stats.languages.is_empty());
        assert!(stats.most_watched.movie.is_none() && stats.most_watched.series.is_none());
        assert!(stats.highest_rated.movie.is_none() && stats.highest_rated.series.is_none());
        // SQL AVG over no rated media is NULL, which stays "no average" rather than 0.
        assert_eq!((stats.average_rating.overall, stats.average_rating.movies, stats.average_rating.series), (None, None, None));
        assert_statement_count(&test_support::db::log(db), 16);
    }

    #[tokio::test]
    async fn stats_omits_other_genre_when_six_genres() {
        test_support::init();
        let db = with_counts(test_support::db::mock(), [1; 6])
            .append_query_results([no_index_rows()])
            .append_query_results([no_index_rows()])
            .append_query_results([(1..=6).map(|i| fixtures::comparative_row(&format!("Genre {i}"), 7 - i)).collect::<Vec<_>>()])
            .append_query_results([no_comparative_rows()])
            .append_query_results([no_index_rows()])
            .append_query_results([no_index_rows()]);
        let db = with_averages(db, [None, None, None])
            .append_query_results([Vec::<entities::image_sizes::Model>::new()])
            .into_connection();

        let stats = stats(&fixtures::user(), &db).await.expect("stats");

        assert_eq!(stats.genres.len(), 6);
        assert!(stats.genres.iter().all(|g| g.name != "Other"));
    }

    #[tokio::test]
    async fn stats_reports_media_as_highest_rated_when_it_is_also_most_watched() {
        test_support::init();
        let movie = media::Model { stars: Some(9.0), ..fixtures::movie_media_model(1, 1) };
        let series = media::Model { stars: Some(8.0), ..fixtures::series_media_model(2, 1) };
        let db = with_counts(test_support::db::mock(), [2, 1, 1, 3, 3, 0])
            .append_query_results([[index_row(movie.clone(), 11, "Alien")]])
            .append_query_results([[index_row(series.clone(), 12, "Bluey")]])
            .append_query_results([no_comparative_rows()])
            .append_query_results([no_comparative_rows()])
            .append_query_results([[index_row(movie, 11, "Alien")]])
            .append_query_results([[index_row(series, 12, "Bluey")]]);
        let db = with_averages(db, [Some(8.5), Some(9.0), Some(8.0)])
            .append_query_results([Vec::<entities::image_sizes::Model>::new()])
            .into_connection();

        let stats = stats(&fixtures::user(), &db).await.expect("stats");

        assert_eq!(stats.most_watched.movie.expect("most watched movie").id, 1);
        assert_eq!(stats.highest_rated.movie.expect("highest rated movie").id, 1);
        assert_eq!(stats.most_watched.series.expect("most watched series").id, 2);
        assert_eq!(stats.highest_rated.series.expect("highest rated series").id, 2);
    }
}
