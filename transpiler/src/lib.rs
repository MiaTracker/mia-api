use std::fmt::{Display, Formatter};
use sea_orm::SelectTwo;
use entities::{media, titles};
use entities::sea_orm_active_enums::MediaType;
use views::media::{PageReq, SearchQuery};
use views::users::CurrentUser;
use crate::constructor::construct;
use crate::parser::parse;

mod parser;
mod lexer;
mod constructor;

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
    pub query: SelectTwo<media::Entity, titles::Entity>,
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