use chrono::{Duration, Utc};
use sea_orm::{ActiveModelTrait, ColumnTrait, Database, DbConn, EntityTrait, NotSet, Order, QueryFilter, QueryOrder};
use sea_orm::ActiveValue::Set;
use entities::prelude::{Media, SyncState};
use entities::sea_orm_active_enums::MediaType;
use entities::{media, sync_state};
use integrations::tmdb;
use log::error;
use sea_orm::prelude::DateTimeWithTimeZone;
use entities::sync_state::Model;
use infrastructure::config;
use views::refresh::RefreshResult;
use crate::infrastructure::SrvErr;
use crate::{movies, series, services};

pub async fn run_refresh() {
    let db_url = config().db.connection_url.clone();
    let conn = Database::connect(db_url.clone()).await
        .expect(format!("Failed to connect to database using connection string \"{}\"", db_url).as_str());

    match refresh(&conn).await {
        Ok(result) => {
            println!("Refresh complete. Updated: {}, Errors: {}", result.updated, result.errors);
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

    // Split [start, now] into 14-day chunks
    let intervals = build_intervals(start, now.date_naive());

    let mut updated = 0usize;
    let mut errors = 0usize;

    for (start_date, end_date) in intervals {

        // --- Movies ---
        let changed_movie_tmdb_ids = match tmdb::services::movies::changed(start_date, end_date).await {
            Ok(ids) => ids,
            Err(e) => {
                error!("Failed to fetch changed movie IDs from TMDB: {}", e);
                errors += 1;
                continue;
            }
        };

        if !changed_movie_tmdb_ids.is_empty() {
            let movie_media: Vec<media::Model> = media::Entity::find()
                .filter(media::Column::TmdbId.is_in(changed_movie_tmdb_ids))
                .filter(media::Column::Type.eq(MediaType::Movie))
                .all(db)
                .await?;

            for med in movie_media {
                let tmdb_id = match med.tmdb_id {
                    Some(id) => id,
                    None => continue,
                };
                let changes = match tmdb::services::movies::property_changes(tmdb_id, start_date, end_date).await {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to fetch changes for movie tmdb_id={}: {}", tmdb_id, e);
                        errors += 1;
                        continue;
                    }
                };
                match movies::apply_changes(&med, &changes, db).await {
                    Ok(_) => updated += 1,
                    Err(e) => {
                        error!("Failed to apply changes for movie media_id={}: {}", med.id, e);
                        errors += 1;
                    }
                }
            }
        }

        // --- Series ---
        let changed_series_tmdb_ids = match tmdb::services::series::changed(start_date, end_date).await {
            Ok(ids) => ids,
            Err(e) => {
                error!("Failed to fetch changed series IDs from TMDB: {}", e);
                errors += 1;
                continue;
            }
        };

        if !changed_series_tmdb_ids.is_empty() {
            let series_media: Vec<media::Model> = media::Entity::find()
                .filter(media::Column::TmdbId.is_in(changed_series_tmdb_ids))
                .filter(media::Column::Type.eq(MediaType::Series))
                .all(db)
                .await?;

            for med in series_media {
                let tmdb_id = match med.tmdb_id {
                    Some(id) => id,
                    None => continue,
                };
                let changes = match tmdb::services::series::property_changes(tmdb_id, start_date, end_date).await {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to fetch changes for series tmdb_id={}: {}", tmdb_id, e);
                        errors += 1;
                        continue;
                    }
                };
                match series::apply_changes(&med, &changes, db).await {
                    Ok(_) => updated += 1,
                    Err(e) => {
                        error!("Failed to apply changes for series media_id={}: {}", med.id, e);
                        errors += 1;
                    }
                }
            }
        }
    }

    // Update last_synced_at
    let state_am = sync_state::ActiveModel {
        id: NotSet,
        synced_at: Set(now.fixed_offset()),
    };
    state_am.insert(db).await?;

    Ok(RefreshResult { updated, errors })
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
