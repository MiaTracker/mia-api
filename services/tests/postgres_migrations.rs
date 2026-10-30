//! The migrations against a real Postgres: they apply to an empty database, and the entities
//! match the schema they produce. Needs Docker: `cargo test -p services --test postgres_migrations -- --ignored`.

mod common;

use common::postgres::database;
use common::seed;
use entities::prelude::*;
use entities::sea_orm_active_enums::{ImageFileType, ImageSource, ImageType};
use entities::{images, manual_image_references, users};
use migration::{Migrator, MigratorTrait};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, NotSet, QueryFilter, Set};

/// The template every test database is cloned from starts empty and is migrated by the bootstrap
/// (one migration at a time); a clone has nothing pending and the seeded admin user.
#[tokio::test]
#[ignore = "requires docker"]
async fn migrations_apply_to_empty_database() {
    let db = database().await;

    assert!(Migrator::get_pending_migrations(&db).await.expect("pending migrations").is_empty());
    let applied = Migrator::get_applied_migrations(&db).await.expect("applied migrations");
    assert_eq!(applied.len(), Migrator::migrations().len());
    let admin = Users::find().filter(users::Column::Username.eq("Admin")).one(&db).await.expect("querying users");
    assert!(admin.is_some_and(|admin| admin.admin), "the seeded admin user should exist");
}

/// Selects every entity once; a column the entity has but the table lacks (or a table that
/// doesn't exist) makes the select fail.
#[tokio::test]
#[ignore = "requires docker"]
async fn entities_match_schema() {
    let db = database().await;
    let mut failures = Vec::new();

    macro_rules! select_each {
        ($($entity:ident),+ $(,)?) => {
            $(
                if let Err(err) = $entity::find().one(&db).await {
                    failures.push(format!("{}: {}", stringify!($entity), err));
                }
            )+
        };
    }
    select_each!(AppTokens, Conversations, Episodes, Exchanges, Genres, ImageSizes, Images, Languages,
        LlmCalls, Logs, ManualImageReferences, Media, MediaGenres, MediaLocks, MediaTags, Messages,
        MovieLocks, Movies, Seasons, Series, SeriesLocks, Sources, SyncState, Tags, Titles, Users,
        Watchlist);

    assert!(failures.is_empty(), "entities that don't match the schema:\n{}", failures.join("\n"));
}

#[tokio::test]
#[ignore = "known bug: manual_image_references.image_id has a foreign key to media.id (meant for media_id)"]
async fn manual_image_references_accepts_reference_when_image_id_is_not_a_media_id() {
    let db = database().await;
    let user = seed::user(&db, 1).await;
    let media = seed::movie(&db, &user, 1, "Movie").await;
    let mut image_ids = Vec::new();
    for n in 0..2 {
        let image = images::ActiveModel {
            id: NotSet,
            path: Set(format!("{n:032x}")),
            source: Set(ImageSource::Manual),
            source_path: Set(None),
            r#type: Set(ImageType::Poster),
            file_type: Set(ImageFileType::Png),
        }.insert(&db).await.expect("inserting image");
        image_ids.push(image.id);
    }
    let image_id = *image_ids.iter().find(|&&id| id != media.id).expect("an image id that is no media id");
    assert!(Media::find_by_id(image_id).one(&db).await.expect("querying media").is_none());

    let result = manual_image_references::ActiveModel { image_id: Set(image_id), media_id: Set(media.id) }
        .insert(&db).await;

    assert!(result.is_ok(), "inserting the reference failed: {:?}", result.err());
}
