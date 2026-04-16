use crate::{assert_request, constants, deserialize};
use crate::infrastructure::{Error, tmdb_get};
use crate::tmdb::views::MultiResults;

pub async fn multi(query: String) -> Result<MultiResults, Error> {
    let uri = format!("{}search/multi?query={}&include_adult=false&language={}&page=1", constants::TMDB_URL, query, constants::TMDB_LANG);
    let resp = tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(MultiResults, resp))
}
