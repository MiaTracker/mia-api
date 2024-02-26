use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, sea_query};
use sea_orm::sea_query::extension::postgres::PgExpr;
use sea_orm::sea_query::{BinOper, Query, SelectStatement, SimpleExpr};
use entities::{logs, media, sea_orm_active_enums, titles};
use entities::prelude::{Media, Titles};
use sea_orm::prelude::Expr;
use entities::sea_orm_active_enums::MediaType;
use crate::{ErrorSource, parser, TranspilationError, TranspilationResult};
use crate::parser::{BinaryExpr, ComparisonOperator, Literal, LogicalExpr, LogicalOperator, TernaryExpr};

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

pub fn construct(query: parser::Query, user_id: i32, media_type: Option<MediaType>) -> Result<TranspilationResult, ConstructionError> {
    let mut primitive = true;

    let mut select = Media::find()
        .filter(media::Column::UserId.eq(user_id))
        .find_also_related(Titles)
        .filter(titles::Column::Primary.eq(true))
        .order_by_asc(titles::Column::Title)
        .distinct();

    if !query.search.is_empty() {
        select = select.filter(media::Column::Id.in_subquery(sea_query::Query::select().distinct()
            .column(titles::Column::MediaId).from(titles::Entity)
            .cond_where(Expr::col(titles::Column::Title).ilike(format!("{}%", query.search))).to_owned()))
    }

    if let Some(media_type) = media_type {
        select = select.filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()));
    }

    if let Some(expr) = query.expr {
        primitive = false;
        select = select.filter(build_expr(expr)?);
    }

    Ok(TranspilationResult {
        query: select,
        is_primitive: primitive,
        name_search: query.search
    })
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

    Ok(expression(&expr.identifier, op_inv, expr.left_literal))
        .and(expression(&expr.identifier, expr.right_operator, expr.right_literal))
}

fn logical_expr(expr: LogicalExpr) -> Result<SimpleExpr, ConstructionError> {
    let op = match expr.operator {
        LogicalOperator::And => { BinOper::And }
        LogicalOperator::Or => { BinOper::Or }
    };

    Ok(build_expr(expr.left)?.binary(op, build_expr(expr.right)?))
}

fn expression(target: &String, op: ComparisonOperator, literal: Literal) -> Result<SimpleExpr, ConstructionError> {
    Ok(
        Expr::col((media::Entity, media::Column::Id)).in_subquery(
            match target.as_str() {
                "stars" => { stars_target(op, literal)? },
                "watched" => { watched_target(op, literal)? },
                "times_watched" => { times_watched_target(op, literal)? },
                "type" => { media_type_target(op, literal)? },
                t => { return Err(ConstructionError::from(format!("Unknown target '{}'", t))) }
            }
        )
    )
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

fn watched_target(op: ComparisonOperator, literal: Literal) -> Result<SelectStatement, ConstructionError> {
    Ok(
        Query::select()
            .columns([(media::Entity, media::Column::Id)])
            .from(media::Entity)
            .left_join(logs::Entity, Expr::col((logs::Entity, logs::Column::MediaId)).equals((media::Entity, media::Column::Id)))
            .group_by_col((media::Entity, media::Column::Id))
            .and_having(Expr::col((logs::Entity, logs::Column::Id)).count().binary(BinOper::GreaterThan, 0).binary(operator(op), literal.to_value::<bool>()?))
            .to_owned()
    )
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

fn media_type_target(op: ComparisonOperator, literal: Literal) -> Result<SelectStatement, ConstructionError> {
    Ok(
        match op {
            ComparisonOperator::Equal => {
                Query::select()
                    .columns([(media::Entity, media::Column::Id)])
                    .from(media::Entity)
                    .and_where(media::Column::Type.eq(literal.to_value::<sea_orm_active_enums::MediaType>()?))
                    .to_owned()
            }
            ComparisonOperator::NotEqual => {
                Query::select()
                    .columns([(media::Entity, media::Column::Id)])
                    .from(media::Entity)
                    .and_where(media::Column::Type.ne(literal.to_value::<sea_orm_active_enums::MediaType>()?))
                    .to_owned()
            },
            x=> { return Err(ConstructionError::from(format!("Cannot compare enum value with operator '{:?}'", x))) }
        }

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