use crate::tmdb::views::{Configuration, Languages};
use crate::{assert_request, constants, deserialize};
use crate::infrastructure::{Error, tmdb_get};

pub async fn languages() -> Result<Vec<Languages>, Error> {
    let uri = constants::TMDB_URL.to_owned() + "configuration/languages";
    let resp = tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(Vec<Languages>, resp))
}

pub async fn details() -> Result<Configuration, Error> {
    let uri = constants::TMDB_URL.to_owned() + "configuration";
    let resp = tmdb_get(&uri).await?;
    assert_request!(resp);
    Ok(deserialize!(Configuration, resp))
}
