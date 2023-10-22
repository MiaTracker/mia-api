use views::tmdb::{MovieDetails, MovieTitles};
use crate::{assert_request, constants};
use crate::infrastructure::{Error, TMDB_HEADERS};
use crate::infrastructure::CLIENT;

pub async fn details(movie_id: i32) -> Result<MovieDetails, Error> {
    let uri = constants::TMDB_URL.to_owned() + "movie/" + movie_id.to_string().as_str() + "?language=" + constants::TMDB_LANG;
    let resp = CLIENT.get(uri).headers(TMDB_HEADERS.clone()).send().await?;
    assert_request!(resp);
    Ok(resp.json::<MovieDetails>().await?)
}

pub async fn alternative_titles(movie_id: i32) -> Result<MovieTitles, Error> {
    let uri = constants::TMDB_URL.to_owned() + "movie/" + movie_id.to_string().as_str() + "/alternative_titles?country=" + constants::TMDB_COUNTRY;
    let resp = CLIENT.get(uri).headers(TMDB_HEADERS.clone()).send().await?;
    assert_request!(resp);
    Ok(resp.json::<MovieTitles>().await?)
}