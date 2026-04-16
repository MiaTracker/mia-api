use chrono::NaiveDate;
use crate::{assert_request, constants, deserialize};
use crate::infrastructure::{CLIENT, Error, TMDB_HEADERS};
use crate::tmdb::views::{ChangedPage, PropertyChanges, SeriesTitles, SeriesDetails, TmdbImages};

pub async fn details(series_id: i32) -> Result<SeriesDetails, Error> {
    let uri = constants::TMDB_URL.to_owned() + "tv/" + series_id.to_string().as_str() + "?languages=" + constants::TMDB_LANG;
    let resp = CLIENT.get(uri).headers(TMDB_HEADERS.clone()).send().await?;
    assert_request!(resp);
    Ok(deserialize!(SeriesDetails, resp))
}

pub async fn alternative_titles(series_id: i32) -> Result<SeriesTitles, Error> {
    let uri = constants::TMDB_URL.to_owned() + "tv/" + series_id.to_string().as_str() + "/alternative_titles";
    let resp = CLIENT.get(uri).headers(TMDB_HEADERS.clone()).send().await?;
    assert_request!(resp);
    Ok(deserialize!(SeriesTitles, resp))
}

pub async fn changed(start_date: NaiveDate, end_date: NaiveDate) -> Result<Vec<i32>, Error> {
    let mut page = 1i32;
    let mut all_ids = Vec::new();
    loop {
        let uri = format!(
            "{}tv/changes?start_date={}&end_date={}&page={}",
            constants::TMDB_URL, start_date, end_date, page
        );
        let resp = CLIENT.get(&uri).headers(TMDB_HEADERS.clone()).send().await?;
        assert_request!(resp);
        let page_data = deserialize!(ChangedPage, resp);
        all_ids.extend(page_data.results.iter().map(|r| r.id));
        if page >= page_data.total_pages {
            break;
        }
        page += 1;
    }
    Ok(all_ids)
}

pub async fn property_changes(series_id: i32, start_date: NaiveDate, end_date: NaiveDate) -> Result<PropertyChanges, Error> {
    let uri = format!(
        "{}tv/{}/changes?start_date={}&end_date={}",
        constants::TMDB_URL, series_id, start_date, end_date
    );
    let resp = CLIENT.get(&uri).headers(TMDB_HEADERS.clone()).send().await?;
    assert_request!(resp);
    Ok(deserialize!(PropertyChanges, resp))
}

pub async fn images(series_id: i32, original_language: &Option<String>) -> Result<TmdbImages, Error> {
    let mut uri = constants::TMDB_URL.to_owned() + "tv/" + series_id.to_string().as_str() + "/images?include_image_language=null%2C" + constants::TMDB_ISO_LANG;
    if let Some(lang) = original_language {
        uri = uri + "%2C" + lang.as_str();
    }
    let resp = CLIENT.get(uri).headers(TMDB_HEADERS.clone()).send().await?;
    assert_request!(resp);
    Ok(deserialize!(TmdbImages, resp))
}