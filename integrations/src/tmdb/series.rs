use crate::{assert_request, constants, deserialize};
use crate::infrastructure::{CLIENT, Error, TMDB_HEADERS};
use views::tmdb::SeriesDetails;

pub async fn details(series_id: i32) -> Result<SeriesDetails, Error> {
    let uri = constants::TMDB_URL.to_owned() + "movie/" + series_id.to_string().as_str() + "?languages=" + constants::TMDB_LANG;
    let resp = CLIENT.get(uri).headers(TMDB_HEADERS.clone()).send().await?;
    assert_request!(resp);
    Ok(deserialize!(SeriesDetails, resp))
}