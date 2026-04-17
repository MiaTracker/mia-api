use chrono::NaiveDate;
use log::error;
use sea_orm_migration::prelude::*;
use integrations::tmdb;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(
            Table::alter()
                .table(Seasons::Table)
                .add_column(ColumnDef::new(Seasons::TmdbId).integer())
                .to_owned(),
        ).await?;

        manager.create_table(
            Table::create()
                .table(Episodes::Table)
                .col(ColumnDef::new(Episodes::Id).integer().primary_key().auto_increment())
                .col(ColumnDef::new(Episodes::SeasonId).integer().not_null())
                .col(ColumnDef::new(Episodes::TmdbId).integer())
                .col(ColumnDef::new(Episodes::EpisodeNumber).integer())
                .col(ColumnDef::new(Episodes::Name).text())
                .col(ColumnDef::new(Episodes::Overview).text())
                .col(ColumnDef::new(Episodes::AirDate).date())
                .col(ColumnDef::new(Episodes::Runtime).integer())
                .col(ColumnDef::new(Episodes::TmdbVoteAverage).float())
                .col(ColumnDef::new(Episodes::StillPath).text())
                .foreign_key(
                    ForeignKey::create()
                        .from(Episodes::Table, Episodes::SeasonId)
                        .to(Seasons::Table, Seasons::Id)
                        .on_update(ForeignKeyAction::Cascade)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        ).await?;

        manager.create_index(
            Index::create()
                .table(Episodes::Table)
                .col(Episodes::SeasonId)
                .col(Episodes::TmdbId)
                .unique()
                .to_owned(),
        ).await?;

        let db = manager.get_connection();
        let builder = manager.get_database_backend();

        integrations::tmdb::services::initialize::initialize().await;

        let query = Query::select()
            .column((Seasons::Table, Seasons::Id))
            .column((Seasons::Table, Seasons::SeasonNumber))
            .column((Media::Table, Media::TmdbId))
            .from(Seasons::Table)
            .inner_join(
                Media::Table,
                Expr::col((Media::Table, Media::Id))
                    .equals((Seasons::Table, Seasons::SeriesId)),
            )
            .cond_where(
                Condition::all()
                    .add(Expr::cust("\"media\".\"type\"::text = 'series'"))
                    .add(Expr::col((Media::Table, Media::TmdbId)).is_not_null())
                    .add(Expr::col((Seasons::Table, Seasons::SeasonNumber)).is_not_null()),
            )
            .to_owned();

        let rows = db.query_all(builder.build(&query)).await?;

        for row in rows {
            let season_db_id = row.try_get_by_index::<i32>(0)?;
            let season_number = row.try_get_by_index::<i32>(1)?;
            let media_tmdb_id = row.try_get_by_index::<i32>(2)?;

            let details = match tmdb::services::series::season_details(media_tmdb_id, season_number).await {
                Ok(d) => d,
                Err(e) => {
                    error!("Failed to fetch season details for media_tmdb_id={} season={}: {}", media_tmdb_id, season_number, e);
                    continue;
                }
            };

            let update = Query::update()
                .table(Seasons::Table)
                .values([(Seasons::TmdbId, details.id.into())])
                .cond_where(Expr::col(Seasons::Id).eq(season_db_id))
                .to_owned();
            if let Err(e) = db.execute(builder.build(&update)).await {
                error!("Failed to update seasons.tmdb_id for season_id={}: {}", season_db_id, e);
            }

            for episode in &details.episodes {
                let air_date: Option<NaiveDate> = episode.air_date.as_deref()
                    .filter(|s| !s.is_empty())
                    .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

                let insert = Query::insert()
                    .into_table(Episodes::Table)
                    .columns([
                        Episodes::SeasonId,
                        Episodes::TmdbId,
                        Episodes::EpisodeNumber,
                        Episodes::Name,
                        Episodes::Overview,
                        Episodes::AirDate,
                        Episodes::Runtime,
                        Episodes::TmdbVoteAverage,
                        Episodes::StillPath,
                    ])
                    .values_panic([
                        season_db_id.into(),
                        episode.id.into(),
                        episode.episode_number.into(),
                        episode.name.clone().into(),
                        episode.overview.clone().into(),
                        air_date.into(),
                        episode.runtime.into(),
                        episode.vote_average.map(|v| v as f64).into(),
                        episode.still_path.clone().into(),
                    ])
                    .on_conflict(
                        OnConflict::columns([Episodes::SeasonId, Episodes::TmdbId])
                            .do_nothing()
                            .to_owned(),
                    )
                    .to_owned();

                if let Err(e) = db.execute(builder.build(&insert)).await {
                    error!("Failed to insert episode tmdb_id={} for season_id={}: {}", episode.id, season_db_id, e);
                }
            }
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Episodes::Table).to_owned()).await?;

        manager.alter_table(
            Table::alter()
                .table(Seasons::Table)
                .drop_column(Seasons::TmdbId)
                .to_owned(),
        ).await
    }
}

#[derive(DeriveIden)]
enum Seasons {
    Table,
    Id,
    SeriesId,
    SeasonNumber,
    TmdbId,
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Id,
    TmdbId,
}

#[derive(DeriveIden)]
enum Episodes {
    Table,
    Id,
    SeasonId,
    TmdbId,
    EpisodeNumber,
    Name,
    Overview,
    AirDate,
    Runtime,
    TmdbVoteAverage,
    StillPath,
}
