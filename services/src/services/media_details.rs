//! The parts of movie and series details that every media has, loaded for many media at once.

use std::collections::{BTreeSet, HashMap, HashSet};
use entities::prelude::{Genres, Languages, Logs, MediaGenres, MediaTags, Sources, Tags, Titles, Watchlist};
use entities::{genres, languages, logs, media, media_genres, media_tags, sources, tags, titles, watchlist};
use infrastructure::config;
use sea_orm::{ColumnTrait, DbConn, EntityTrait, QueryFilter, QueryOrder};
use views::genres::Genre;
use views::languages::Language;
use views::logs::Log;
use views::sources::Source;
use views::tags::Tag;
use views::titles::AlternativeTitle;

use crate::infrastructure::SrvErr;

/// What `movies::details` and `series::details` show of a media besides its own row, its
/// movie/series row, its locks and its images. `pub` so the Postgres tests can check
/// [`load_parts`] directly.
pub struct MediaParts {
    /// The primary title, or `config().media.unset_title` when the media has none.
    pub title: String,
    pub alternative_titles: Vec<AlternativeTitle>,
    pub original_language: Option<Language>,
    pub genres: Vec<Genre>,
    pub tags: Vec<Tag>,
    pub sources: Vec<Source>,
    /// Each log's `source` is the name of its source.
    pub logs: Vec<Log>,
    pub on_watchlist: bool,
}

/// The [`MediaParts`] of every media in `media`, keyed by media id. One query each for titles,
/// languages, genres, tags, sources, logs and the watchlist, each over all of `media`: 7 queries
/// whatever its length. The languages query is skipped when no media has an original language
/// (then 6), and an empty `media` issues none. Every list is ordered by id.
///
/// The caller passes media already filtered by user (and type); no ownership is checked here.
/// `pub` so the Postgres tests can check the batched queries directly.
pub async fn load_parts(media: &[media::Model], db: &DbConn) -> Result<HashMap<i32, MediaParts>, SrvErr> {
    if media.is_empty() {
        return Ok(HashMap::new());
    }
    let media_ids: Vec<i32> = media.iter().map(|m| m.id).collect();

    let titles = Titles::find().filter(titles::Column::MediaId.is_in(media_ids.clone()))
        .order_by_asc(titles::Column::Id).all(db).await?;
    let language_ids: BTreeSet<&String> = media.iter().filter_map(|m| m.original_language.as_ref()).collect();
    let languages: HashMap<String, languages::Model> = if language_ids.is_empty() {
        HashMap::new()
    } else {
        Languages::find().filter(languages::Column::Iso6391.is_in(language_ids)).all(db).await?
            .into_iter().map(|l| (l.iso6391.clone(), l)).collect()
    };
    let genres = MediaGenres::find().filter(media_genres::Column::MediaId.is_in(media_ids.clone()))
        .find_also_related(Genres).order_by_asc(genres::Column::Id).all(db).await?;
    let tags = MediaTags::find().filter(media_tags::Column::MediaId.is_in(media_ids.clone()))
        .find_also_related(Tags).order_by_asc(tags::Column::Id).all(db).await?;
    let sources = Sources::find().filter(sources::Column::MediaId.is_in(media_ids.clone()))
        .order_by_asc(sources::Column::Id).all(db).await?;
    let logs = Logs::find().filter(logs::Column::MediaId.is_in(media_ids.clone()))
        .order_by_asc(logs::Column::Id).all(db).await?;
    let watchlist: HashSet<i32> = Watchlist::find().filter(watchlist::Column::MediaId.is_in(media_ids)).all(db).await?
        .into_iter().map(|w| w.media_id).collect();

    let mut parts: HashMap<i32, MediaParts> = media.iter().map(|m| (m.id, MediaParts {
        title: config().media.unset_title.clone(),
        alternative_titles: Vec::new(),
        original_language: m.original_language.as_ref()
            .and_then(|id| languages.get(id)).map(|l| Language::from(l.clone())),
        genres: Vec::new(),
        tags: Vec::new(),
        sources: Vec::new(),
        logs: Vec::new(),
        on_watchlist: watchlist.contains(&m.id),
    })).collect();

    for title in &titles {
        if let Some(p) = parts.get_mut(&title.media_id) {
            if title.primary {
                p.title = title.title.clone();
            } else {
                p.alternative_titles.push(AlternativeTitle::from(title));
            }
        }
    }
    for (media_genre, genre) in &genres {
        if let (Some(p), Some(genre)) = (parts.get_mut(&media_genre.media_id), genre) {
            p.genres.push(Genre::from(genre));
        }
    }
    for (media_tag, tag) in &tags {
        if let (Some(p), Some(tag)) = (parts.get_mut(&media_tag.media_id), tag) {
            p.tags.push(Tag::from(tag));
        }
    }
    for source in &sources {
        if let Some(p) = parts.get_mut(&source.media_id) {
            p.sources.push(Source::from(source));
        }
    }
    for log in &logs {
        if let Some(p) = parts.get_mut(&log.media_id) {
            let mut view = Log::from(log);
            if let Some(source) = p.sources.iter().find(|s| s.id == log.source_id) {
                view.source = source.name.clone();
            }
            p.logs.push(view);
        }
    }

    Ok(parts)
}

#[cfg(test)]
mod tests {
    use entities::sea_orm_active_enums::MediaType;
    use super::*;
    use crate::test_support;
    use crate::test_support::db::assert_statement_count;
    use crate::test_support::fixtures;

    fn names<T>(items: &[T], name: fn(&T) -> &str) -> Vec<&str> {
        items.iter().map(name).collect()
    }

    #[tokio::test]
    async fn load_parts_issues_seven_queries_when_given_three_media() {
        test_support::init();
        let media = vec![
            media::Model { original_language: Some("en".into()), ..fixtures::movie_media_model(1, 1) },
            media::Model { original_language: Some("en".into()), ..fixtures::movie_media_model(2, 1) },
            fixtures::movie_media_model(3, 1),
        ];
        let db = test_support::db::mock()
            .append_query_results([[
                fixtures::title_model(1, 1, "Alien", true),
                fixtures::title_model(2, 1, "Alien: The Eighth Passenger", false),
                fixtures::title_model(3, 2, "Scream", true),
            ]])
            .append_query_results([[fixtures::language_model("en", "English")]])
            .append_query_results([[
                (fixtures::media_genre_model(1, 3), Some(fixtures::genre_model(3, "Science Fiction", MediaType::Movie))),
                (fixtures::media_genre_model(2, 5), Some(fixtures::genre_model(5, "Horror", MediaType::Movie))),
            ]])
            .append_query_results([[
                (fixtures::media_tag_model(1, 4), Some(fixtures::tag_model(4, "classic"))),
                (fixtures::media_tag_model(3, 4), Some(fixtures::tag_model(4, "classic"))),
            ]])
            .append_query_results([[
                fixtures::source_model(9, 1, "Blu-ray"),
                fixtures::source_model(10, 3, "Netflix"),
            ]])
            .append_query_results([[
                fixtures::log_model(1, 1, 9),
                fixtures::log_model(2, 3, 10),
            ]])
            .append_query_results([[fixtures::watchlist_model(3)]])
            .into_connection();

        let parts = load_parts(&media, &db).await.ok().expect("load_parts should succeed");

        assert_eq!(parts.len(), 3);
        let alien = &parts[&1];
        assert_eq!(alien.title, "Alien");
        assert_eq!(names(&alien.alternative_titles, |t| &t.title), vec!["Alien: The Eighth Passenger"]);
        assert_eq!(alien.original_language.as_ref().map(|l| l.english_name.as_str()), Some("English"));
        assert_eq!(names(&alien.genres, |g| &g.name), vec!["Science Fiction"]);
        assert_eq!(names(&alien.tags, |t| &t.name), vec!["classic"]);
        assert_eq!(names(&alien.sources, |s| &s.name), vec!["Blu-ray"]);
        assert_eq!(names(&alien.logs, |l| &l.source), vec!["Blu-ray"]);
        assert!(!alien.on_watchlist);
        let scream = &parts[&2];
        assert_eq!(scream.title, "Scream");
        assert!(scream.alternative_titles.is_empty());
        assert_eq!(scream.original_language.as_ref().map(|l| l.english_name.as_str()), Some("English"));
        assert_eq!(names(&scream.genres, |g| &g.name), vec!["Horror"]);
        assert!(scream.tags.is_empty());
        assert!(scream.sources.is_empty());
        assert!(scream.logs.is_empty());
        assert!(!scream.on_watchlist);
        let untitled = &parts[&3];
        assert_eq!(untitled.title, "Unknown");
        assert!(untitled.original_language.is_none());
        assert!(untitled.genres.is_empty());
        assert_eq!(names(&untitled.tags, |t| &t.name), vec!["classic"]);
        assert_eq!(names(&untitled.sources, |s| &s.name), vec!["Netflix"]);
        assert_eq!(names(&untitled.logs, |l| &l.source), vec!["Netflix"]);
        assert!(untitled.on_watchlist);
        assert_statement_count(&test_support::db::log(db), 7);
    }

    #[tokio::test]
    async fn load_parts_issues_no_queries_when_given_no_media() {
        test_support::init();
        let db = test_support::db::mock().into_connection();

        let parts = load_parts(&[], &db).await.ok().expect("load_parts should succeed");

        assert!(parts.is_empty());
        assert_statement_count(&test_support::db::log(db), 0);
    }
}
