use crate::tmdb::views::{MovieDetails, TmdbImages, MovieTitles};
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

pub async fn images(movie_id: i32, original_language: &Option<String>) -> Result<TmdbImages, Error> {
    let mut uri = constants::TMDB_URL.to_owned() + "movie/" + movie_id.to_string().as_str() + "/images?include_image_language=null%2C" + constants::TMDB_ISO_LANG;
    if let Some(lang) = original_language {
        uri = uri + "%2C" + lang.as_str();
    }
    let resp = CLIENT.get(uri).headers(TMDB_HEADERS.clone()).send().await?;
    assert_request!(resp);
    Ok(deserialize!(TmdbImages, resp))
}