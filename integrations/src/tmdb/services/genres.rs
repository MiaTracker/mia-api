use crate::tmdb::views::GenreList;
use crate::{assert_request, constants, deserialize};
use crate::infrastructure::{Error, tmdb_get};

pub async fn movie() -> Result<GenreList, Error> {
    let uri = constants::TMDB_URL.to_owned() + "genre/movie/list?language=" + constants::TMDB_LANG;
    let resp = tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(GenreList, resp))
}

pub async fn tv() -> Result<GenreList, Error> {
    let uri = constants::TMDB_URL.to_owned() + "genre/tv/list?language=" + constants::TMDB_LANG;
    let resp = tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(GenreList, resp))
}
