use std::fmt::Write;
use sea_orm::Iden;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::{Func, SimpleExpr};
use sea_orm::sea_query::extension::postgres::PgExpr;
use crate::{functions, titles};

pub struct Right;

impl Iden for Right {
    fn unquoted(&self, s: &mut dyn Write) {
        write!(s, "RIGHT").unwrap();
    }
}

pub fn default_media_sort() -> SimpleExpr {
    SimpleExpr::Case(Box::new(Expr::case(Expr::col(titles::Column::Title).ilike("a %"), Func::cust(functions::Right).arg(Func::lower(Expr::col(titles::Column::Title))).arg(-2))
        .case(Expr::col(titles::Column::Title).ilike("the %"), Func::cust(functions::Right).arg(Func::lower(Expr::col(titles::Column::Title))).arg(-4))
        .finally(Func::lower(Expr::col(titles::Column::Title)))
    ))
}