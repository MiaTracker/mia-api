use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, IntoActiveModel, ModelTrait, NotSet, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set};
use sea_orm::sea_query::Query;
use entities::{functions, image_sizes, media, titles, watchlist};
use entities::prelude::{ImageSizes, Media, Titles, Watchlist};
use entities::traits::linked::MediaPosters;
use views::media::{MediaIndex, PageReq, SearchQuery, SearchResults};
use views::users::CurrentUser;
use crate::infrastructure::{Clock, SrvErr};
use crate::services;

pub async fn add(media_id: i32, user: &CurrentUser, db: &DbConn, clock: &dyn Clock) -> Result<bool, SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::UserId.eq(user.id)).one(db).await?;
    if media.is_none() {
        return Err(SrvErr::NotFound);
    }

    let already_added = watchlist::Entity::find().filter(watchlist::Column::MediaId.eq(media_id)).count(db).await? != 0;
    if already_added {
        return Ok(false);
    }

    let am = watchlist::ActiveModel {
        media_id: Set(media_id),
        assessment: NotSet,
        date_added: Set(clock.now().date_naive()),
    };
    am.insert(db).await?;

    let media = media.unwrap();
    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.into_active_model();
        media_am.bot_controllable = Set(bot_controllable);
        media_am.update(db).await?;
    }

    Ok(true)
}

pub async fn index(page_req: PageReq, user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let media = Media::find().right_join(Watchlist)
        .filter(media::Column::UserId.eq(user.id))
        .find_also_linked(MediaPosters)
        .find_also_related(Titles)
        .filter(titles::Column::Primary.eq(true))
        .order_by_asc(functions::default_media_sort())
        .offset(page_req.offset).limit(page_req.limit).all(db).await?;

    let poster_ids: Vec<i32> = media.iter().filter_map(|p| p.0.poster_image_id).collect();
    let poster_sizes = ImageSizes::find()
        .filter(image_sizes::Column::ImageId.is_in(poster_ids)).all(db).await?;

    let indexes = services::media::build_media_indexes(media, poster_sizes);
    Ok(indexes)
}

pub async fn search(query: SearchQuery, page_req: PageReq, user: &CurrentUser, db: &DbConn) -> Result<SearchResults, SrvErr> {
    let res = transpiler::transpile(query, page_req, user, None);
    if res.is_err() {
        return Ok(SearchResults {
            indexes: vec![],
            external: vec![],
            query_valid: false,
        });
    }
    let res = res.ok().unwrap();

    let media = res.query.filter(media::Column::Id.in_subquery(Query::select()
            .column(watchlist::Column::MediaId).from(Watchlist).to_owned()))
        .all(db).await?;

    let poster_ids: Vec<i32> = media.iter().filter_map(|p| p.0.poster_image_id).collect();
    let poster_sizes = ImageSizes::find()
        .filter(image_sizes::Column::ImageId.is_in(poster_ids)).all(db).await?;


    let indexes = services::media::build_media_indexes(media, poster_sizes);

    Ok(SearchResults {
        indexes,
        external: vec![],
        query_valid: true,
    })
}


pub async fn remove(media_id: i32, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let watch = watchlist::Entity::find().filter(watchlist::Column::MediaId.eq(media_id))
        .inner_join(Media).filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if watch.is_none() {
        return Err(SrvErr::NotFound);
    }
    watch.unwrap().delete(db).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::Value;
    use entities::sea_orm_active_enums::MediaType;
    use entities::image_sizes;
    use crate::test_support;
    use crate::test_support::db::{assert_no_statement_starting_with, assert_statement_count, assert_wrote, deleted};
    use crate::test_support::fixtures;
    use crate::test_support::fixed_clock;

    #[tokio::test]
    async fn add_sets_date_added_from_clock() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::media_model(10, user.id, MediaType::Movie)]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([vec![fixtures::watchlist_model(10)]])
            .into_connection();

        let added = add(10, &user, &db, &fixed_clock(2026, 3, 15, 23, 59, 0)).await.ok().expect("add should succeed");

        assert!(added);
        let log = test_support::db::log(db);
        let today = test_support::date(2026, 3, 15);
        assert_wrote(&log, "INSERT INTO \"watchlist\"", &[Value::from(10), Value::from(today)]);
    }

    #[tokio::test]
    async fn add_returns_false_when_already_on_watchlist() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::media_model(10, user.id, MediaType::Movie)]])
            .append_query_results([[fixtures::count_row(1)]])
            .into_connection();

        let added = add(10, &user, &db, &fixed_clock(2026, 3, 15, 23, 59, 0)).await.ok().expect("add should succeed");

        assert!(!added);
        assert_no_statement_starting_with(&test_support::db::log(db), "INSERT");
    }

    #[tokio::test]
    async fn add_returns_not_found_when_media_missing() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media::Model>::new()])
            .into_connection();

        let result = add(10, &fixtures::user(), &db, &fixed_clock(2026, 3, 15, 23, 59, 0)).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
    }

    #[tokio::test]
    async fn remove_deletes_watchlist_entry() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::watchlist_model(10)]])
            .append_exec_results([deleted()])
            .into_connection();

        let result = remove(10, &fixtures::user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, "DELETE FROM \"watchlist\"", &[Value::from(10)]);
    }

    #[tokio::test]
    async fn remove_returns_not_found_when_not_on_watchlist() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::watchlist::Model>::new()])
            .into_connection();

        let result = remove(10, &fixtures::user(), &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
    }

    #[tokio::test]
    async fn index_returns_page() {
        test_support::init();
        let with_poster = media::Model { poster_image_id: Some(2), stars: Some(4.5), ..fixtures::media_model(1, 1, MediaType::Movie) };
        let without_poster = fixtures::media_model(3, 1, MediaType::Series);
        let db = test_support::db::mock()
            .append_query_results([vec![
                fixtures::media_index_row(with_poster, Some(fixtures::image_model(2)), Some(fixtures::title_model(1, 1, "Alien", true))),
                fixtures::media_index_row(without_poster, None, Some(fixtures::title_model(3, 3, "Bluey", true))),
            ]])
            .append_query_results([[fixtures::image_size_model(1, 2, 300, 450), fixtures::image_size_model(2, 2, 100, 150)]])
            .into_connection();

        let page = PageReq { offset: Some(20), limit: Some(10) };
        let result = index(page, &fixtures::user(), &db).await.ok().expect("index should succeed");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, 1);
        assert_eq!(result[0].title, "Alien");
        assert_eq!(result[0].stars, Some(4.5));
        let poster = result[0].poster.as_ref().expect("first media has a poster");
        assert_eq!(poster.path, "/image-2.jpg");
        assert_eq!(poster.sizes.len(), 2);
        assert_eq!(result[1].id, 3);
        assert_eq!(result[1].title, "Bluey");
        assert!(result[1].poster.is_none());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert!(log[0].sql.contains("\"watchlist\""), "the page is read from the watchlist: {}", log[0].sql);
        assert_wrote(&log, "SELECT", &[Value::from(1), Value::from(20u64), Value::from(10u64)]);
    }

    #[tokio::test]
    async fn index_returns_empty_when_watchlist_empty() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([Vec::<image_sizes::Model>::new()])
            .into_connection();

        let result = index(PageReq { offset: None, limit: None }, &fixtures::user(), &db).await.ok().expect("index should succeed");

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn search_filters_to_watchlist() {
        test_support::init();
        let media = media::Model { poster_image_id: Some(2), ..fixtures::media_model(1, 1, MediaType::Movie) };
        let db = test_support::db::mock()
            .append_query_results([vec![
                fixtures::media_index_row(media, Some(fixtures::image_model(2)), Some(fixtures::title_model(1, 1, "Alien", true))),
            ]])
            .append_query_results([[fixtures::image_size_model(1, 2, 300, 450)]])
            .into_connection();
        let query = SearchQuery { query: "alien".to_string(), genres: None, only_watched: false, min_stars: None, sort_by: Default::default() };

        let result = search(query, PageReq { offset: None, limit: Some(10) }, &fixtures::user(), &db).await.ok().expect("search should succeed");

        assert!(result.query_valid);
        assert!(result.external.is_empty());
        assert_eq!(result.indexes.len(), 1);
        assert_eq!(result.indexes[0].id, 1);
        assert_eq!(result.indexes[0].title, "Alien");
        assert_eq!(result.indexes[0].poster.as_ref().expect("media has a poster").sizes.len(), 1);
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert!(log[0].sql.contains("\"watchlist\""), "the search is restricted to the watchlist: {}", log[0].sql);
        assert_wrote(&log, "SELECT", &[Value::from(1), Value::from(10u64)]);
    }

    #[tokio::test]
    async fn search_returns_invalid_query_result_when_query_unparseable() {
        test_support::init();
        let db = test_support::db::mock().into_connection();
        // The part after " : " is an expression; "@" is not a valid token.
        let query = SearchQuery { query: "alien : @".to_string(), genres: None, only_watched: false, min_stars: None, sort_by: Default::default() };

        let result = search(query, PageReq { offset: None, limit: None }, &fixtures::user(), &db).await.ok().expect("search should succeed");

        assert!(!result.query_valid);
        assert!(result.indexes.is_empty());
        assert!(result.external.is_empty());
        assert_statement_count(&test_support::db::log(db), 0);
    }
}
