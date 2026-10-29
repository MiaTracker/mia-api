use crate::{assert_request, constants, deserialize};
use crate::infrastructure::Error;
use crate::tmdb::TmdbClient;
use crate::tmdb::views::MultiResults;

pub(crate) async fn multi(client: &TmdbClient, query: String) -> Result<MultiResults, Error> {
    let uri = format!("{}search/multi?query={}&include_adult=false&language={}&page=1", client.base_url(), query, constants::TMDB_LANG);
    let resp = client.tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(MultiResults, resp))
}
