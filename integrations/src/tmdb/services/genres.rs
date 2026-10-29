use crate::tmdb::views::GenreList;
use crate::{assert_request, constants, deserialize};
use crate::infrastructure::Error;
use crate::tmdb::TmdbClient;

pub(crate) async fn movie(client: &TmdbClient) -> Result<GenreList, Error> {
    let uri = client.base_url().to_owned() + "genre/movie/list?language=" + constants::TMDB_LANG;
    let resp = client.tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(GenreList, resp))
}

pub(crate) async fn tv(client: &TmdbClient) -> Result<GenreList, Error> {
    let uri = client.base_url().to_owned() + "genre/tv/list?language=" + constants::TMDB_LANG;
    let resp = client.tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(GenreList, resp))
}
