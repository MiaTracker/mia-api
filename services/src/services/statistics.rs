use futures::try_join;
use sea_orm::{ColumnTrait, DbConn, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};
use entities::{logs, media, titles};
use entities::prelude::{Logs, Media, Titles};
use entities::sea_orm_active_enums::MediaType;
use views::statistics::{LogStats, MediaStats, MostWatchedStats, Stats};
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;

pub async fn stats(user: &CurrentUser, db: &DbConn) -> Result<Stats, SrvErr> {
    let media_count_future = Media::find().filter(media::Column::UserId.eq(user.id)).count(db);
    let movie_count_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Movie)).count(db);
    let series_count_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Series)).count(db);

    let log_count_future = Media::find().filter(media::Column::UserId.eq(user.id)).inner_join(Logs).count(db);
    let completed_log_count_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(logs::Column::Completed.eq(true)).inner_join(Logs).count(db);
    let uncompleted_log_count_future = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(logs::Column::Completed.eq(false)).inner_join(Logs).count(db);

    let movie = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Movie))
        .inner_join(Logs)
        .group_by(media::Column::Id).group_by(titles::Column::Id).order_by_desc(logs::Column::Id.count()).order_by_desc(media::Column::Stars).limit(1)
        .find_also_related(Titles).filter(titles::Column::Primary.eq(true)).one(db);
    let series = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Series))
        .inner_join(Logs)
        .group_by(media::Column::Id).group_by(titles::Column::Id).order_by_desc(logs::Column::Id.count()).order_by_desc(media::Column::Stars).limit(1)
        .find_also_related(Titles).filter(titles::Column::Primary.eq(true)).one(db);


    let (media_count, movie_count, series_count,
        log_count, completed_log_count, uncompleted_log_count,
        movie, series) = try_join!(media_count_future, movie_count_future, series_count_future,
        log_count_future, completed_log_count_future, uncompleted_log_count_future,
        movie, series)?;

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
        most_watched: MostWatchedStats {
            movie: movie.map(|m| {
                crate::services::media::build_media_index(m)
            }),
            series: series.map(|s| {
                crate::services::media::build_media_index(s)
            }),
        },
    })
}