use chrono::NaiveDate;
use crate::tmdb::views::{ChangedPage, PropertyChanges, MovieDetails, TmdbImages, MovieTitles};
use crate::{assert_request, constants, deserialize};
use crate::infrastructure::Error;
use crate::tmdb::TmdbClient;

pub(crate) async fn details(client: &TmdbClient, movie_id: i32) -> Result<MovieDetails, Error> {
    let uri = client.base_url().to_owned() + "movie/" + movie_id.to_string().as_str() + "?languages=" + constants::TMDB_LANG;
    let resp = client.tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(MovieDetails, resp))
}

pub(crate) async fn alternative_titles(client: &TmdbClient, movie_id: i32) -> Result<MovieTitles, Error> {
    let uri = client.base_url().to_owned() + "movie/" + movie_id.to_string().as_str() + "/alternative_titles?country=" + constants::TMDB_COUNTRY;
    let resp = client.tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(MovieTitles, resp))
}

pub(crate) async fn changed(client: &TmdbClient, start_date: NaiveDate, end_date: NaiveDate) -> Result<Vec<i32>, Error> {
    let mut page = 1i32;
    let mut all_ids = Vec::new();
    loop {
        let uri = format!(
            "{}movie/changes?start_date={}&end_date={}&page={}",
            client.base_url(), start_date, end_date, page
        );
        let resp = client.tmdb_get(&uri).await?;
        assert_request!(resp);
        let page_data = deserialize!(ChangedPage, resp);
        all_ids.extend(page_data.results.iter().map(|r| r.id));
        if page_data.page >= page_data.total_pages {
            break;
        }
        if page_data.total_pages > 500 {
            return Err(Error {
                message: "Result had more than 500 pages.".to_string(),
                status_code: None,
                source: None,
            })
        }
        page = page_data.page + 1;
    }
    Ok(all_ids)
}

pub(crate) async fn property_changes(client: &TmdbClient, movie_id: i32, start_date: NaiveDate, end_date: NaiveDate) -> Result<PropertyChanges, Error> {
    let uri = format!(
        "{}movie/{}/changes?start_date={}&end_date={}",
        client.base_url(), movie_id, start_date, end_date
    );
    let resp = client.tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(PropertyChanges, resp))
}

pub(crate) async fn images(client: &TmdbClient, movie_id: i32, original_language: &Option<String>) -> Result<TmdbImages, Error> {
    let mut uri = client.base_url().to_owned() + "movie/" + movie_id.to_string().as_str() + "/images?include_image_language=null%2C" + constants::TMDB_ISO_LANG;
    if let Some(lang) = original_language {
        uri = uri + "%2C" + lang.as_str();
    }
    let resp = client.tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(TmdbImages, resp))
}
