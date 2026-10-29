use crate::tmdb::views::{Configuration, Languages};
use crate::{assert_request, deserialize};
use crate::infrastructure::Error;
use crate::tmdb::TmdbClient;

pub(crate) async fn languages(client: &TmdbClient) -> Result<Vec<Languages>, Error> {
    let uri = client.base_url().to_owned() + "configuration/languages";
    let resp = client.tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(Vec<Languages>, resp))
}

pub(crate) async fn details(client: &TmdbClient) -> Result<Configuration, Error> {
    let uri = client.base_url().to_owned() + "configuration";
    let resp = client.tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(Configuration, resp))
}
