use sea_orm::{ColumnTrait, EntityTrait, JoinType, Order, QueryFilter, QueryOrder, QuerySelect};
use sea_orm::prelude::Expr;
use sea_orm::sea_query::{Alias, BinOper, Cond, Func, Query, SelectStatement, SimpleExpr, UnionType};
use sea_orm::sea_query::extension::postgres::PgExpr;

use entities::{functions, genres, languages, logs, media, media_genres, media_tags, movies, series, sources, tags, titles};
use entities::functions::DatePart;
use entities::prelude::{Media, Titles};
use entities::sea_orm_active_enums::MediaType;
use views::media::{PageReq, SearchQuery, SortTarget};

use crate::{ErrorSource, ExternalId, parser, TranspilationError, TranspilationResult};
use crate::parser::{BinaryExpr, ComparisonOperator, Literal, LogicalExpr, LogicalOperator, SortDirection, TernaryExpr};

macro_rules! assert_eq {
    ($op:ident) => {
        if $op != ComparisonOperator::Equal {
            return Err(ConstructionError::from(format!("Operator {} is not supported with this target", $op)));
        }
    };
}

#[derive(Debug)]
pub struct ConstructionError {
    pub message: String
}

impl From<String> for ConstructionError {
    fn from(value: String) -> Self {
        ConstructionError {
            message: value,
        }
    }
}

impl From<&str> for ConstructionError {
    fn from(value: &str) -> Self {
        ConstructionError {
            message: value.to_string(),
        }
    }
}

impl Into<TranspilationError> for ConstructionError {
    fn into(self) -> TranspilationError {
        TranspilationError {
            source: ErrorSource::Construction,
            message: self.message,
        }
    }
}

trait FromLiteral {
    fn from_literal(literal: Literal) -> Result<Option<Self>, ConstructionError> where Self: Sized;
}

impl Literal {
    fn to_value<T: FromLiteral>(self) -> Result<Option<T>, ConstructionError> {
        T::from_literal(self)
    }
}

impl FromLiteral for i32 {
    fn from_literal(literal: Literal) -> Result<Option<Self>, ConstructionError> {
        match literal {
            Literal::False => { Ok(Some(0)) }
            Literal::True => { Ok(Some(1)) }
            Literal::Null => { Ok(None) }
            Literal::Int(i) => { Ok(Some(i)) }
            Literal::Float(f) => { Ok(Some(f.trunc() as i32)) }
            Literal::String(s) => {
                match s.parse::<i32>() {
                    Ok(r) => { Ok(Some(r)) }
                    Err(_) => { Err(ConstructionError::from(format!("Expected integer, found '{}'", s))) }
                }
            }
        }
    }
}

impl FromLiteral for f32 {
    fn from_literal(literal: Literal) -> Result<Option<Self>, ConstructionError> {
        match literal {
            Literal::False => { Ok(Some(0f32)) }
            Literal::True => { Ok(Some(1f32)) }
            Literal::Null => { Ok(None) }
            Literal::Int(i) => { Ok(Some(i as f32)) }
            Literal::Float(f) => { Ok(Some(f)) }
            Literal::String(s) => {
                match s.parse::<f32>() {
                    Ok(r) => { Ok(Some(r)) }
                    Err(_) => { Err(ConstructionError::from(format!("Expected float, found '{}'", s))) }
                }
            }
        }
    }
}

impl FromLiteral for String {
    fn from_literal(literal: Literal) -> Result<Option<Self>, ConstructionError> {
        match literal {
            Literal::Null => { Ok(None) }
            Literal::Int(i) => { Ok(Some(i.to_string())) }
            Literal::Float(f) => { Ok(Some(f.to_string())) }
            Literal::String(s) => { Ok(Some(s)) },
            l => { Err(ConstructionError::from(format!("Expected string, found '{:?}'", l))) }
        }
    }
}

impl FromLiteral for bool {
    fn from_literal(literal: Literal) -> Result<Option<Self>, ConstructionError> where Self: Sized {
        match literal {
            Literal::False => { Ok(Some(false)) }
            Literal::True => { Ok(Some(true)) }
            Literal::Null => { Ok(None) }
            Literal::Int(i) => { Ok(Some(i.is_positive())) }
            Literal::Float(f) => { Ok(Some(f.is_sign_positive())) }
            Literal::String(s) => { Ok(Some(!s.is_empty())) }
        }
    }
}

impl FromLiteral for MediaType {
    fn from_literal(literal: Literal) -> Result<Option<Self>, ConstructionError> where Self: Sized {
        match literal {
            Literal::String(s) => {
                match s.to_lowercase().as_str() {
                    "movie" => Ok(Some(MediaType::Movie)),
                    "series" => Ok(Some(MediaType::Series)),
                    x => { Err(ConstructionError::from(format!("Expected media type, found '{}'", x))) }
                }
            },
            x => { Err(ConstructionError::from(format!("Expected media type, found '{:?}'", x)))}
        }
    }
}

pub fn construct(query: parser::Query, search: SearchQuery, page_req: PageReq, user_id: i32, media_type: Option<MediaType>) -> Result<TranspilationResult, ConstructionError> {
    let mut primitive = true;

    let mut select = Media::find()
        .filter(media::Column::UserId.eq(user_id))
        .find_also_related(Titles)
        .filter(titles::Column::Primary.eq(true))
        .offset(page_req.offset).limit(page_req.limit);

    let mut custom_sort;
    if let Some(sort_target) = query.sort_target {
        let dir = match sort_target.direction {
            None => { Order::Asc }
            Some(d) => {
                match d {
                    SortDirection::Ascending => { Order::Asc }
                    SortDirection::Descending => { Order::Desc }
                }
            }
        };

        select = select.order_by(sort(sort_target.identifier)?, dir.clone());
        custom_sort = true;
    } else {
        custom_sort = false;
    }

    let (naive_expr, sort_expr, prim) = build_naive_search(search)?;
    select = select.filter(naive_expr);
    primitive &= prim;
    if !custom_sort {
        if let Some(sort_expr) = sort_expr {
            select = select.order_by_desc(sort_expr);
            custom_sort = true;
        }
    }
    if !custom_sort {
        select = select.order_by_asc(functions::default_media_sort())
    } else {
        primitive = false;
    }

    select = select.order_by_asc(Expr::col((titles::Entity, titles::Column::Title)));

    if !query.search.is_empty() {
        select = select.filter(media::Column::Id.in_subquery(Query::select().distinct()
            .column(titles::Column::MediaId).from(titles::Entity)
            .cond_where(Expr::col(titles::Column::Title).ilike(format!("{}%", query.search))
                .or(Expr::col(titles::Column::Title).ilike(format!("the {}%", query.search)))
                .or(Expr::col(titles::Column::Title).ilike(format!("a {}%", query.search))))
            .to_owned())
        )
    }

    if let Some(media_type) = media_type {
        select = select.filter(media::Column::Type.eq::<MediaType>(media_type.into()));
    }

    let mut external_id: Option<ExternalId> = None;
    if let Some(expr) = query.expr {
        if primitive {
            if let Some(id) = try_get_external_id_expr(&expr) {
                if query.search.is_empty() {
                    external_id = Some(id);
                } else { primitive = false; }
            } else { primitive = false; }
        } else {
            primitive = false;
        }
        select = select.filter(build_expr(expr)?);
    }

    Ok(TranspilationResult {
        query: select,
        is_primitive: primitive,
        name_search: query.search,
        external_id
    })
}

fn try_get_external_id_expr(expr: &parser::Expr) -> Option<ExternalId> {
    match expr {
        parser::Expr::Logical(l) => {
            if l.operator != LogicalOperator::And { return None; }
            let fr = try_get_external_val_bin_expr(&l.left, None, None)?;
            if fr.0.is_none() && fr.1.is_none() { return None; }
            let sr = try_get_external_val_bin_expr(&l.right, fr.0, fr.1)?;
            if sr.0.is_none() || sr.0.is_none() { return None; }
            Some(ExternalId { tmdb_id: sr.0.unwrap(), r#type: sr.1.unwrap() })
        },
        _ => None
    }
}

fn try_get_external_val_bin_expr(expr: &parser::Expr, id: Option<i32>, t: Option<views::media::MediaType>) -> Option<(Option<i32>, Option<views::media::MediaType>)> {
    match expr {
        parser::Expr::Binary(b) => {
            if b.operator != ComparisonOperator::Equal { return None; }
            let lower = b.identifier.to_lowercase();
            if lower == "tmdb_id" {
                if id.is_none() {
                    if let Ok(num) = b.literal.clone().to_value::<i32>() {
                        if let Some(tid) = num {
                            return Some((Some(tid), t));
                        } else {
                            return None;
                        }
                    } else { return None; }
                } else { return None; }
            } else if lower == "type" {
                if t.is_none() {
                    if let Ok(typ) = b.literal.clone().to_value::<MediaType>() {
                        if let Some(tp) = typ {
                            return Some((id, Some(tp.into())));
                        } else {
                            return None;
                        }
                    } else { return None; }
                } else { return None; }
            } else { None }
        },
        _ => None
    }
}

fn build_naive_search(search: SearchQuery) -> Result<(SimpleExpr, Option<SimpleExpr>, bool), ConstructionError> {
    let mut primitive = true;

    let mut expr = Expr::val(1).eq(1);
    if let Some(genres) = search.genres {
        for genre in genres {
            expr = expr.and(Expr::col((media::Entity, media::Column::Id)).in_subquery(
                has_genre_target(ComparisonOperator::Equal, Literal::String(genre))?)
            );
            primitive = false;
        }
    }

    if search.only_watched {
        expr = expr.and(Expr::col((media::Entity, media::Column::Id)).in_subquery(
            watched_target(ComparisonOperator::Equal, Literal::True)?)
        );
        primitive = false;
    }

    if let Some(min_stars) = search.min_stars {
        expr = expr.and(Expr::col((media::Entity, media::Column::Id)).in_subquery(
            stars_target(ComparisonOperator::GreaterEqual, Literal::Float(min_stars))?)
        );
        primitive = false;
    }

    let sort_expr = match search.sort_by {
        SortTarget::Title => { None }
        SortTarget::Stars => { Some(stars_sort_target()) }
        SortTarget::TimesWatched => { Some(times_watched_sort_target()) }
    }.and_then(|e| Some(SimpleExpr::SubQuery(None, Box::new(e.into_sub_query_statement()))));


    Ok((expr, sort_expr, primitive))
}

fn build_expr(expr: parser::Expr) -> Result<SimpleExpr, ConstructionError> {
    match expr {
        parser::Expr::Binary(e) => {
            binary_expr(*e)
        },
        parser::Expr::Ternary(e) => {
            ternary_expr(*e)
        },
        parser::Expr::Logical(e) => {
            logical_expr(*e)
        },
    }
}

fn binary_expr(expr: BinaryExpr) -> Result<SimpleExpr, ConstructionError> {
    expression(&expr.identifier, expr.operator, expr.literal)
}

fn ternary_expr(expr: TernaryExpr) -> Result<SimpleExpr, ConstructionError> {
    let op_inv = match expr.left_operator {
        ComparisonOperator::Less => { ComparisonOperator::Greater }
        ComparisonOperator::LessEqual => { ComparisonOperator::GreaterEqual }
        ComparisonOperator::Greater => { ComparisonOperator::Less }
        ComparisonOperator::GreaterEqual => { ComparisonOperator::LessEqual }
        op => { op }
    };

    if expr.left_literal == Literal::Null || expr.right_literal == Literal::Null {
        return Err(ConstructionError::from("Ternary expression should not check against Null!"))
    }

    Ok(expression(&expr.identifier, op_inv, expr.left_literal)?
        .and(expression(&expr.identifier, expr.right_operator, expr.right_literal)?))
}

fn logical_expr(expr: LogicalExpr) -> Result<SimpleExpr, ConstructionError> {
    let op = match expr.operator {
        LogicalOperator::And => { BinOper::And }
        LogicalOperator::Or => { BinOper::Or }
    };

    Ok(build_expr(expr.left)?.binary(op, build_expr(expr.right)?))
}

fn expression(tar: &String, op: ComparisonOperator, literal: Literal) -> Result<SimpleExpr, ConstructionError> {
    Ok(
        if op == ComparisonOperator::NotEqual {
            Expr::col((media::Entity, media::Column::Id)).not_in_subquery(
                target(tar, ComparisonOperator::Equal, literal)?
            )
        } else {
            Expr::col((media::Entity, media::Column::Id)).in_subquery(
                target(tar, op, literal)?
            )
        }
    )
}

fn target(target: &String, op: ComparisonOperator, literal: Literal) -> Result<SelectStatement, ConstructionError> {
    match target.to_lowercase().as_str() {
        "stars" => { stars_target(op, literal) }
        "watched" => { watched_target(op, literal) }
        "times_watched" => { times_watched_target(op, literal) }
        "type" => { media_type_target(op, literal) }
        "has_source" => { has_source_target(op, literal) }
        "number_of_sources" => { number_of_sources_target(op, literal) }
        "has_source_with_name" => { has_source_with_name_target(op, literal) }
        "has_tag" => { has_tag_target(op, literal) }
        "has_genre" => { has_genre_target(op, literal) }
        "language" => { language_target(op, literal) }
        "id" => { id_target(op, literal) }
        "tmdb_id" => { tmdb_id_target(op, literal) }
        "year" => { year_target(op, literal) }
        _ => { return Err(ConstructionError::from(format!("Unknown target '{}'", target))) }
    }
}

fn stars_target(op: ComparisonOperator, literal: Literal) -> Result<SelectStatement, ConstructionError> {
    Ok(
        Query::select()
            .columns([media::Column::Id])
            .from(media::Entity)
            .and_where(Expr::col((media::Entity, media::Column::Stars)).binary(operator(op), literal.to_value::<f32>()?))
            .to_owned()
    )
}

fn stars_sort_target() -> SelectStatement {
    Query::select()
        .expr(Expr::col((Alias::new("ord_media"), media::Column::Stars)).if_null(-1))
        .from_as(media::Entity, Alias::new("ord_media"))
        .and_where(Expr::col((Alias::new("ord_media"), media::Column::Id)).equals((media::Entity, media::Column::Id)))
        .to_owned()
}

fn watched_target(op: ComparisonOperator, literal: Literal) -> Result<SelectStatement, ConstructionError> {
    assert_eq!(op);
    Ok(
        Query::select()
            .columns([(media::Entity, media::Column::Id)])
            .from(media::Entity)
            .left_join(logs::Entity, Expr::col((logs::Entity, logs::Column::MediaId)).equals((media::Entity, media::Column::Id)))
            .group_by_col((media::Entity, media::Column::Id))
            .and_having(Expr::col((logs::Entity, logs::Column::Id)).count().binary(BinOper::GreaterThan, 0).eq(literal.to_value::<bool>()?))
            .to_owned()
    )
}

fn watched_sort_target() -> SelectStatement {
    Query::select()
        .expr_as(Expr::col((Alias::new("ord_logs"), logs::Column::Id)).count().binary(BinOper::GreaterThan, 0), Alias::new("count"))
        .from_as(media::Entity, Alias::new("ord_media"))
        .join_as(JoinType::LeftJoin, logs::Entity, Alias::new("ord_logs"), Expr::col((Alias::new("ord_logs"), logs::Column::MediaId)).equals((Alias::new("ord_media"), media::Column::Id)))
        .and_where(Expr::col((Alias::new("ord_media"), media::Column::Id)).equals((media::Entity, media::Column::Id)))
        .group_by_col((Alias::new("ord_logs"), logs::Column::MediaId))
        .to_owned()
}

fn times_watched_target(op: ComparisonOperator, literal: Literal) -> Result<SelectStatement, ConstructionError> {
    Ok(
        Query::select()
            .columns([(media::Entity, media::Column::Id)])
            .from(media::Entity)
            .left_join(logs::Entity, Expr::col((logs::Entity, logs::Column::MediaId)).equals((media::Entity, media::Column::Id)))
            .group_by_col((media::Entity, media::Column::Id))
            .and_having(Expr::col((logs::Entity, logs::Column::Id)).count().binary(operator(op), literal.to_value::<i32>()?))
            .to_owned()
    )
}

fn times_watched_sort_target() -> SelectStatement {
    Query::select()
        .expr_as(Expr::col((Alias::new("ord_logs"), logs::Column::Id)).count(), Alias::new("count"))
        .from_as(media::Entity, Alias::new("ord_media"))
        .join_as(JoinType::LeftJoin, logs::Entity, Alias::new("ord_logs"), Expr::col((Alias::new("ord_logs"), logs::Column::MediaId)).equals((Alias::new("ord_media"), media::Column::Id)))
        .and_where(Expr::col((Alias::new("ord_media"), media::Column::Id)).equals((media::Entity, media::Column::Id)))
        .group_by_col((Alias::new("ord_logs"), logs::Column::MediaId))
        .to_owned()
}

fn media_type_target(op: ComparisonOperator, literal: Literal) -> Result<SelectStatement, ConstructionError> {
    assert_eq!(op);
    Ok(
        Query::select()
            .columns([(media::Entity, media::Column::Id)])
            .from(media::Entity)
            .and_where(media::Column::Type.eq(literal.to_value::<MediaType>()?))
            .to_owned()
    )
}

fn media_type_sort_target() -> SelectStatement {
    Query::select()
        .columns([(Alias::new("ord_media"), media::Column::Type)])
        .from_as(media::Entity, Alias::new("ord_media"))
        .and_where(Expr::col((Alias::new("ord_media"), media::Column::Id)).equals((media::Entity, media::Column::Id)))
        .to_owned()
}

fn number_of_sources_target(op: ComparisonOperator, literal: Literal) -> Result<SelectStatement, ConstructionError> {
    Ok(
        Query::select()
            .columns([(media::Entity, media::Column::Id)])
            .from(media::Entity)
            .left_join(sources::Entity, Expr::col((sources::Entity, sources::Column::MediaId)).equals((media::Entity, media::Column::Id)))
            .group_by_col((media::Entity, media::Column::Id))
            .and_having(Expr::col((sources::Entity, sources::Column::Id)).count().binary(operator(op), literal.to_value::<i32>()?))
            .to_owned()
    )
}

fn number_of_sources_sort_target() -> SelectStatement {
    Query::select()
        .expr_as(Expr::col((Alias::new("ord_sources"), sources::Column::Id)).count(), Alias::new("count"))
        .from_as(media::Entity, Alias::new("ord_media"))
        .join_as(JoinType::LeftJoin, sources::Entity, Alias::new("ord_sources"), Expr::col((Alias::new("ord_sources"), sources::Column::MediaId)).equals((Alias::new("ord_media"), media::Column::Id)))
        .and_where(Expr::col((Alias::new("ord_media"), media::Column::Id)).equals((media::Entity, media::Column::Id)))
        .group_by_col((Alias::new("ord_sources"), sources::Column::MediaId))
        .to_owned()
}

fn has_source_target(op: ComparisonOperator, literal: Literal) -> Result<SelectStatement, ConstructionError> {
    assert_eq!(op);
    Ok(
        Query::select()
            .columns([(media::Entity, media::Column::Id)])
            .from(media::Entity)
            .left_join(sources::Entity, Expr::col((sources::Entity, sources::Column::MediaId)).equals((media::Entity, media::Column::Id)))
            .group_by_col((media::Entity, media::Column::Id))
            .and_having(Expr::col((sources::Entity, sources::Column::Id)).count().binary(BinOper::GreaterThan, 0).eq(literal.to_value::<bool>()?))
            .to_owned()
    )
}

fn has_source_sort_target() -> SelectStatement {
    Query::select()
        .expr_as(Expr::col((Alias::new("ord_sources"), sources::Column::Id)).count().binary(BinOper::GreaterThan, 0), Alias::new("count"))
        .from_as(media::Entity, Alias::new("ord_media"))
        .join_as(JoinType::LeftJoin, sources::Entity, Alias::new("ord_sources"), Expr::col((Alias::new("ord_sources"), sources::Column::MediaId)).equals((Alias::new("ord_media"), media::Column::Id)))
        .and_where(Expr::col((Alias::new("ord_media"), media::Column::Id)).equals((media::Entity, media::Column::Id)))
        .group_by_col((Alias::new("ord_sources"), sources::Column::MediaId))
        .to_owned()
}

fn has_source_with_name_target(op: ComparisonOperator, literal: Literal) -> Result<SelectStatement, ConstructionError> {
    assert_eq!(op);
    Ok(
        Query::select()
            .columns([(sources::Entity, sources::Column::MediaId)])
            .from(sources::Entity)
            .and_where(Expr::expr(Func::lower(Expr::col((sources::Entity, sources::Column::Name)))).eq(literal.to_value::<String>()?.map(|s| s.to_lowercase())))
            .to_owned()
    )
}

fn has_tag_target(op: ComparisonOperator, literal: Literal) -> Result<SelectStatement, ConstructionError> {
    assert_eq!(op);
    Ok(
        Query::select()
            .columns([(media_tags::Entity, media_tags::Column::MediaId)])
            .from(media_tags::Entity)
            .inner_join(tags::Entity, Expr::col((tags::Entity, tags::Column::Id)).equals((media_tags::Entity, media_tags::Column::TagId)))
            .and_where(Expr::expr(Func::lower(Expr::col((tags::Entity, tags::Column::Name)))).eq(literal.to_value::<String>()?.map(|s| s.to_lowercase())))
            .to_owned()
    )
}

fn has_genre_target(op: ComparisonOperator, literal: Literal) -> Result<SelectStatement, ConstructionError> {
    assert_eq!(op);
    Ok(
        Query::select()
            .columns([(media_genres::Entity, media_genres::Column::MediaId)])
            .from(media_genres::Entity)
            .inner_join(genres::Entity, Expr::col((genres::Entity, genres::Column::Id)).equals((media_genres::Entity, media_genres::Column::GenreId)))
            .and_where(Expr::expr(Func::lower(Expr::col((genres::Entity, genres::Column::Name)))).eq(literal.to_value::<String>()?.map(|s| s.to_lowercase())))
            .to_owned()
    )
}

fn language_target(op: ComparisonOperator, literal: Literal) -> Result<SelectStatement, ConstructionError> {
    assert_eq!(op);
    let name = literal.to_value::<String>()?.map(|s| s.to_lowercase());
    Ok(
        Query::select()
            .columns([(media::Entity, media::Column::Id)])
            .from(media::Entity)
            .inner_join(languages::Entity, Expr::col((languages::Entity, languages::Column::Iso6391)).equals((media::Entity, media::Column::OriginalLanguage)))
            .cond_where(
                Cond::any()
                    .add(Expr::expr(Func::lower(Expr::col((languages::Entity, languages::Column::EnglishName)))).eq(name.clone()))
                    .add(Expr::expr(Func::lower(Expr::col((languages::Entity, languages::Column::Name)))).eq(name.clone()))
                    .add(Expr::expr(Func::lower(Expr::col((languages::Entity, languages::Column::Iso6391)))).eq(name))
            )
            .to_owned()
    )
}

fn id_target(op: ComparisonOperator, literal: Literal) -> Result<SelectStatement, ConstructionError> {
    let id = literal.to_value::<i32>()?;
    let operator = operator(op);
    Ok(
        Query::select()
            .columns([(media::Entity, media::Column::Id)])
            .from(media::Entity)
            .cond_where(Expr::col((media::Entity, media::Column::Id)).binary(operator, id))
            .to_owned()
    )
}

fn tmdb_id_target(op: ComparisonOperator, literal: Literal) -> Result<SelectStatement, ConstructionError> {
    let tmdb_id = literal.to_value::<i32>()?;
    let operator = operator(op);
    Ok(
        Query::select()
            .columns([(media::Entity, media::Column::Id)])
            .from(media::Entity)
            .cond_where(Expr::col((media::Entity, media::Column::TmdbId)).binary(operator, tmdb_id))
            .to_owned()
    )
}

fn year_target(op: ComparisonOperator, literal: Literal) -> Result<SelectStatement, ConstructionError> {
    let year = literal.to_value::<i32>()?;
    let operator = operator(op);
    Ok(
        Query::select()
            .columns([(movies::Entity, movies::Column::Id)])
            .from(movies::Entity)
            .cond_where(Expr::expr(Func::cust(DatePart).arg("Year").arg(Expr::col((movies::Entity, movies::Column::ReleaseDate)))).binary(operator, year))
            .union(UnionType::All, Query::select()
                .columns([(series::Entity, series::Column::Id)])
                .from(series::Entity)
                .cond_where(Expr::expr(Func::cust(DatePart).arg("Year").arg(Expr::col((series::Entity, series::Column::FirstAirDate)))).binary(operator, year))
                .to_owned()
            )
            .to_owned()
    )
}

fn operator(operator: ComparisonOperator) -> BinOper {
    match operator {
        ComparisonOperator::Equal => { BinOper::Equal }
        ComparisonOperator::NotEqual => { BinOper::NotEqual }
        ComparisonOperator::Less => { BinOper::SmallerThan }
        ComparisonOperator::LessEqual => { BinOper::SmallerThanOrEqual }
        ComparisonOperator::Greater => { BinOper::GreaterThan }
        ComparisonOperator::GreaterEqual => { BinOper::GreaterThanOrEqual }
    }
}

fn sort(target: String) -> Result<SimpleExpr, ConstructionError> {
    Ok(SimpleExpr::SubQuery(None, Box::new(
        match target.to_lowercase().as_str() {
            "stars" => { stars_sort_target() },
            "watched" => { watched_sort_target() },
            "times_watched" => { times_watched_sort_target() },
            "type" => { media_type_sort_target() },
            "has_source" => { has_source_sort_target() },
            "number_of_sources" => { number_of_sources_sort_target() }
            _ => { return Err(ConstructionError::from(format!("Unknown sort target '{}'", target))) }
        }.into_sub_query_statement()
    )))
}