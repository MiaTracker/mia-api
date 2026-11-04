use crate::constructor::construct;
use crate::parser::parse;
use entities::sea_orm_active_enums::MediaType;
use entities::{images, media, titles};
use sea_orm::SelectThree;
use std::fmt::{Display, Formatter};
use views::media::{PageReq, SearchQuery};
use views::users::CurrentUser;

mod parser;
mod lexer;
mod constructor;

pub use constructor::{FILTER_FIELDS, SORT_TARGETS};

pub enum ErrorSource {
    Lexing,
    Parsing,
    Construction
}

impl Display for ErrorSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorSource::Lexing => { f.write_str("Lexing") }
            ErrorSource::Parsing => { f.write_str("Parsing") }
            ErrorSource::Construction => { f.write_str("Construction") }
        }
    }
}

pub struct TranspilationError {
    pub source: ErrorSource,
    pub message: String
}

pub struct TranspilationResult {
    pub query: SelectThree<media::Entity, images::Entity, titles::Entity>,
    pub name_search: String,
    pub external_id: Option<ExternalId>,
    pub is_primitive: bool
}

pub struct ExternalId {
    pub tmdb_id: i32,
    pub r#type: views::media::MediaType
}

pub fn transpile(search: SearchQuery, page_req: PageReq, user: &CurrentUser, media_type: Option<MediaType>)
                 -> Result<TranspilationResult, Vec<TranspilationError>> {
    let q = parse(search.query.clone());
    if q.lexing_errs.is_some() || q.parsing_errs.is_some() {
        let mut errs = Vec::new();
        if let Some(lexing_errs) = q.lexing_errs {
            errs = lexing_errs.iter().map(|e| { e.into() }).collect();
        }
        if let Some(parsing_errs) = q.parsing_errs {
            let es = parsing_errs.iter().map(|e| e.into());
            for err in es {
                errs.push(err)
            }
        }
        return Err(errs)
    }

    match construct(q, search, page_req, user.id, media_type) {
        Ok(s) => { Ok(s) }
        Err(err) => { Err(vec![err.into()]) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::prelude::Uuid;

    fn user() -> CurrentUser {
        CurrentUser { id: 1, uuid: Uuid::nil(), email: "tester@example.test".into(), username: "tester".into(), admin: false, though_bot: false }
    }

    fn search(query: &str) -> SearchQuery {
        SearchQuery { query: query.into(), genres: None, only_watched: false, min_stars: None, sort_by: Default::default() }
    }

    #[test]
    fn transpile_orders_by_media_id_last_when_sorting_by_default_or_custom_target() {
        use sea_orm::{DbBackend, QueryTrait};

        for query in ["", "alien", " :  : stars desc", " :  : release_date asc"] {
            let result = transpile(search(query), PageReq { offset: None, limit: None }, &user(), None).ok().expect("query transpiles");

            let sql = result.query.build(DbBackend::Postgres).to_string();

            let order_by = sql.rsplit("ORDER BY").next().expect("query has ORDER BY").trim_end();
            assert!(order_by.ends_with(r#""media"."id" ASC"#), "query {query:?} does not end with the media id: {order_by}");
        }
    }

    #[test]
    fn transpile_returns_error_when_filter_has_trailing_text() {
        let result = transpile(search(" : stars > 5 rated = true"), PageReq { offset: None, limit: None }, &user(), None);

        assert!(result.is_err());
    }
}
