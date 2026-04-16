use chrono::NaiveDate;
use crate::tmdb::views::{ChangedPage, PropertyChanges, MovieDetails, TmdbImages, MovieTitles};
use crate::{assert_request, constants, deserialize};
use crate::infrastructure::{Error, TMDB_HEADERS};
use crate::infrastructure::CLIENT;

pub async fn details(movie_id: i32) -> Result<MovieDetails, Error> {
    let uri = constants::TMDB_URL.to_owned() + "movie/" + movie_id.to_string().as_str() + "?languages=" + constants::TMDB_LANG;
    let resp = CLIENT.get(uri).headers(TMDB_HEADERS.clone()).send().await?;
    assert_request!(resp);
    Ok(deserialize!(MovieDetails, resp))
}

pub async fn alternative_titles(movie_id: i32) -> Result<MovieTitles, Error> {
    let uri = constants::TMDB_URL.to_owned() + "movie/" + movie_id.to_string().as_str() + "/alternative_titles?country=" + constants::TMDB_COUNTRY;
    let resp = CLIENT.get(uri).headers(TMDB_HEADERS.clone()).send().await?;
    assert_request!(resp);
    Ok(deserialize!(MovieTitles, resp))
}

pub async fn changed(start_date: NaiveDate, end_date: NaiveDate) -> Result<Vec<i32>, Error> {
    let mut page = 1i32;
    let mut all_ids = Vec::new();
    loop {
        let uri = format!(
            "{}movie/changes?start_date={}&end_date={}&page={}",
            constants::TMDB_URL, start_date, end_date, page
        );
        let resp = CLIENT.get(&uri).headers(TMDB_HEADERS.clone()).send().await?;
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

pub async fn property_changes(movie_id: i32, start_date: NaiveDate, end_date: NaiveDate) -> Result<PropertyChanges, Error> {
    let uri = format!(
        "{}movie/{}/changes?start_date={}&end_date={}",
        constants::TMDB_URL, movie_id, start_date, end_date
    );
    let resp = CLIENT.get(&uri).headers(TMDB_HEADERS.clone()).send().await?;
    assert_request!(resp);
    Ok(deserialize!(PropertyChanges, resp))
}

pub async fn images(movie_id: i32, original_language: &Option<String>) -> Result<TmdbImages, Error> {
    let mut uri = constants::TMDB_URL.to_owned() + "movie/" + movie_id.to_string().as_str() + "/images?include_image_language=null%2C" + constants::TMDB_ISO_LANG;
    if let Some(lang) = original_language {
        uri = uri + "%2C" + lang.as_str();
    }
    let resp = CLIENT.get(uri).headers(TMDB_HEADERS.clone()).send().await?;
    assert_request!(resp);
    Ok(deserialize!(TmdbImages, resp))
}