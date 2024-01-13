use crate::tmdb::views::{Configuration, Languages};
use crate::{assert_request, constants, deserialize};
use crate::infrastructure::{CLIENT, Error, TMDB_HEADERS};

pub async fn languages() -> Result<Vec<Languages>, Error> {
    let uri = constants::TMDB_URL.to_owned() + "configuration/languages";
    let resp = CLIENT.get(uri).headers(TMDB_HEADERS.clone()).send().await?;
    assert_request!(resp);
    Ok(deserialize!(Vec<Languages>, resp))
}

pub async fn details() -> Result<Configuration, Error> {
    let uri = constants::TMDB_URL.to_owned() + "configuration";
    let resp = CLIENT.get(uri).headers(TMDB_HEADERS.clone()).send().await?;
    assert_request!(resp);
    Ok(deserialize!(Configuration, resp))
}