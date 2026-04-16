use chrono::{Duration, Utc};
use futures::StreamExt;
use sea_orm::{ActiveModelTrait, ColumnTrait, Database, DbConn, EntityTrait, FromQueryResult, NotSet, Order, QueryFilter, QueryOrder, QuerySelect};
use sea_orm::ActiveValue::Set;
use entities::prelude::{Media, SyncState};
use entities::sea_orm_active_enums::MediaType;
use entities::{media, sync_state};
use integrations::tmdb;
use log::{error, info};
use infrastructure::config;
use views::refresh::RefreshResult;
use crate::infrastructure::SrvErr;
use crate::{movies, series};

pub async fn run_refresh() {
    crate::infrastructure::initialize().await;

    let db_url = config().db.connection_url.clone();
    let conn = Database::connect(db_url.clone()).await
        .expect(format!("Failed to connect to database using connection string \"{}\"", db_url).as_str());

    match refresh(&conn).await {
        Ok(result) => {
            println!("Refresh complete. Updated: {}, Errors: {}", result.updates, result.errors);
        }
        Err(err) => {
            eprintln!("Refresh failed: {}", err);
            std::process::exit(1);
        }
    }
}

pub async fn refresh(db: &DbConn) -> Result<RefreshResult, SrvErr> {
    let last_state = SyncState::find().order_by(sync_state::Column::SyncedAt, Order::Desc).one(db).await?;

    let now = Utc::now();

    let start = match last_state {
        None => {
            let m = Media::find().order_by(media::Column::DateAdded, Order::Asc).one(db).await?;
            m.map(|m| m.date_added).unwrap_or(now.date_naive())
        }
        Some(s) => { s.synced_at.date_naive() }
    };
    let end_date = now.date_naive();

    async fn fetch_all_tmdb_ids(media_type: MediaType, db: &DbConn) -> Result<Vec<i32>, SrvErr> {
        #[derive(FromQueryResult)]
        struct IdEntity { pub tmdb_id: Option<i32> }

        Ok(Media::find()
            .select_only().columns([media::Column::TmdbId])
            .filter(media::Column::Type.eq(media_type))
            .into_model::<IdEntity>()
            .all(db)
            .await?
            .into_iter()
            .filter_map(|e| e.tmdb_id)
            .collect())
    }

    let within_window = end_date - start < Duration::days(14);
    let (movie_tmdb_ids, series_tmdb_ids) = futures::try_join!(
        async {
            if within_window {
                match tmdb::services::movies::changed(start, end_date).await {
                    Ok(ids) => Ok(ids),
                    Err(e) => {
                        info!("Failed to fetch changed movie IDs from TMDB: {} Falling back to full update", e);
                        fetch_all_tmdb_ids(MediaType::Movie, db).await
                    }
                }
            } else {
                fetch_all_tmdb_ids(MediaType::Movie, db).await
            }
        },
        async {
            if within_window {
                match tmdb::services::series::changed(start, end_date).await {
                    Ok(ids) => Ok(ids),
                    Err(e) => {
                        info!("Failed to fetch changed series IDs from TMDB: {} Falling back to full update", e);
                        fetch_all_tmdb_ids(MediaType::Series, db).await
                    }
                }
            } else {
                fetch_all_tmdb_ids(MediaType::Series, db).await
            }
        }
    )?;

    // Split [start, now] into 14-day chunks
    let intervals = build_intervals(start, now.date_naive());

    let mut updates = 0usize;
    let mut errors = 0usize;

    for (start_date, end_date) in intervals {
        let mut futures: Vec<std::pin::Pin<Box<dyn Future<Output = (usize, usize)> + Send + '_>>> = Vec::new();

        if !movie_tmdb_ids.is_empty() {
            let movie_media: Vec<media::Model> = media::Entity::find()
                .filter(media::Column::TmdbId.is_in(movie_tmdb_ids.clone()))
                .filter(media::Column::Type.eq(MediaType::Movie))
                .all(db)
                .await?;

            for med in movie_media {
                futures.push(Box::pin(async move {
                    let tmdb_id = match med.tmdb_id {
                        Some(id) => id,
                        None => return (0, 0),
                    };
                    let changes = match tmdb::services::movies::property_changes(tmdb_id, start_date, end_date).await {
                        Ok(c) => c,
                        Err(e) => {
                            error!("Failed to fetch changes for movie tmdb_id={}: {}", tmdb_id, e);
                            return (0, 1);
                        }
                    };
                    match movies::apply_changes(&med, &changes, db).await {
                        Ok(_) => (1, 0),
                        Err(e) => {
                            error!("Failed to apply changes for movie media_id={}: {}", med.id, e);
                            (0, 1)
                        }
                    }
                }));
            }
        }

        if !series_tmdb_ids.is_empty() {
            let series_media: Vec<media::Model> = media::Entity::find()
                .filter(media::Column::TmdbId.is_in(series_tmdb_ids.clone()))
                .filter(media::Column::Type.eq(MediaType::Series))
                .all(db)
                .await?;

            for med in series_media {
                futures.push(Box::pin(async move {
                    let tmdb_id = match med.tmdb_id {
                        Some(id) => id,
                        None => return (0, 0),
                    };
                    let changes = match tmdb::services::series::property_changes(tmdb_id, start_date, end_date).await {
                        Ok(c) => c,
                        Err(e) => {
                            error!("Failed to fetch changes for series tmdb_id={}: {}", tmdb_id, e);
                            return (0, 1);
                        }
                    };
                    match series::apply_changes(&med, &changes, db).await {
                        Ok(_) => (1, 0),
                        Err(e) => {
                            error!("Failed to apply changes for series media_id={}: {}", med.id, e);
                            (0, 1)
                        }
                    }
                }));
            }
        }

        let results: Vec<(usize, usize)> = futures::stream::iter(futures)
            .buffer_unordered(15)
            .collect()
            .await;
        for (u, e) in results {
            updates += u;
            errors += e;
        }
    }

    // Update last_synced_at
    let state_am = sync_state::ActiveModel {
        id: NotSet,
        synced_at: Set(now.fixed_offset()),
    };
    state_am.insert(db).await?;

    Ok(RefreshResult { updates, errors })
}

/// Split [start, end] into non-overlapping 14-day windows.
fn build_intervals(
    start: chrono::NaiveDate,
    end: chrono::NaiveDate,
) -> Vec<(chrono::NaiveDate, chrono::NaiveDate)> {
    let mut intervals = Vec::new();
    let mut current = start;
    while current < end {
        let next = (current + Duration::days(14)).min(end);
        intervals.push((current, next));
        current = next;
    }
    intervals
}
