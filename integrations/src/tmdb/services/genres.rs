use crate::tmdb::views::GenreList;
use crate::{assert_request, constants, deserialize};
use crate::infrastructure::{CLIENT, Error, TMDB_HEADERS};

pub async fn movie() -> Result<GenreList, Error> {
    let uri = constants::TMDB_URL.to_owned() + "genre/movie/list?language=" + constants::TMDB_LANG;
    let resp = CLIENT.get(uri).headers(TMDB_HEADERS.clone()).send().await?;
    assert_request!(resp);
    Ok(deserialize!(GenreList, resp))
}

pub async fn tv() -> Result<GenreList, Error> {
    let uri = constants::TMDB_URL.to_owned() + "genre/tv/list?language=" + constants::TMDB_LANG;
    let resp = CLIENT.get(uri).headers(TMDB_HEADERS.clone()).send().await?;
    assert_request!(resp);
    Ok(deserialize!(GenreList, resp))
}