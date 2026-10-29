use chrono::NaiveDate;
use crate::{assert_request, constants, deserialize};
use crate::infrastructure::Error;
use crate::tmdb::TmdbClient;
use crate::tmdb::views::{ChangedPage, PropertyChanges, SeasonDetails, SeriesTitles, SeriesDetails, TmdbImages};

pub(crate) async fn details(client: &TmdbClient, series_id: i32) -> Result<SeriesDetails, Error> {
    let uri = client.base_url().to_owned() + "tv/" + series_id.to_string().as_str() + "?languages=" + constants::TMDB_LANG;
    let resp = client.tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(SeriesDetails, resp))
}

pub(crate) async fn alternative_titles(client: &TmdbClient, series_id: i32) -> Result<SeriesTitles, Error> {
    let uri = client.base_url().to_owned() + "tv/" + series_id.to_string().as_str() + "/alternative_titles";
    let resp = client.tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(SeriesTitles, resp))
}

pub(crate) async fn changed(client: &TmdbClient, start_date: NaiveDate, end_date: NaiveDate) -> Result<Vec<i32>, Error> {
    let mut page = 1i32;
    let mut all_ids = Vec::new();
    loop {
        let uri = format!(
            "{}tv/changes?start_date={}&end_date={}&page={}",
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

pub(crate) async fn property_changes(client: &TmdbClient, series_id: i32, start_date: NaiveDate, end_date: NaiveDate) -> Result<PropertyChanges, Error> {
    let uri = format!(
        "{}tv/{}/changes?start_date={}&end_date={}",
        client.base_url(), series_id, start_date, end_date
    );
    let resp = client.tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(PropertyChanges, resp))
}

pub(crate) async fn season_details(client: &TmdbClient, series_id: i32, season_number: i32) -> Result<SeasonDetails, Error> {
    let uri = format!("{}tv/{}/season/{}", client.base_url(), series_id, season_number);
    let resp = client.tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(SeasonDetails, resp))
}

pub(crate) async fn season_property_changes(client: &TmdbClient, season_id: i32, start_date: NaiveDate, end_date: NaiveDate) -> Result<PropertyChanges, Error> {
    let uri = format!(
        "{}tv/season/{}/changes?start_date={}&end_date={}",
        client.base_url(), season_id, start_date, end_date
    );
    let resp = client.tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(PropertyChanges, resp))
}

pub(crate) async fn episode_property_changes(client: &TmdbClient, episode_id: i32, start_date: NaiveDate, end_date: NaiveDate) -> Result<PropertyChanges, Error> {
    let uri = format!(
        "{}tv/episode/{}/changes?start_date={}&end_date={}",
        client.base_url(), episode_id, start_date, end_date
    );
    let resp = client.tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(PropertyChanges, resp))
}

pub(crate) async fn images(client: &TmdbClient, series_id: i32, original_language: &Option<String>) -> Result<TmdbImages, Error> {
    let mut uri = client.base_url().to_owned() + "tv/" + series_id.to_string().as_str() + "/images?include_image_language=null%2C" + constants::TMDB_ISO_LANG;
    if let Some(lang) = original_language {
        uri = uri + "%2C" + lang.as_str();
    }
    let resp = client.tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(TmdbImages, resp))
}
