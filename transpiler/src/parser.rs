use crate::{ErrorSource, TranspilationError};
use crate::lexer::{LexingError, scan};

pub struct Query {
    pub search: String,
    pub expr: Option<Expr>,
    pub sort_target: Option<SortTarget>,
    pub lexing_errs: Option<Vec<LexingError>>,
    pub parsing_errs: Option<Vec<ParseError>>
}

pub struct SortTarget {
    pub identifier: String,
    pub direction: Option<SortDirection>
}

pub enum SortDirection {
    Ascending,
    Descending
}

#[derive(Debug)]
pub enum Expr {
    Binary(Box<BinaryExpr>),
    Ternary(Box<TernaryExpr>),
    Logical(Box<LogicalExpr>)
}

#[derive(Debug)]
pub struct BinaryExpr {
    pub identifier: String,
    pub operator: ComparisonOperator,
    pub literal: Literal
}

#[derive(Debug)]
pub struct TernaryExpr {
    pub left_literal: Literal,
    pub left_operator: ComparisonOperator,
    pub identifier: String,
    pub right_operator: ComparisonOperator,
    pub right_literal: Literal
}

#[derive(Debug)]
pub struct LogicalExpr {
    pub left: Expr,
    pub operator: LogicalOperator,
    pub right: Expr
}

#[derive(Debug)]
pub struct Token {
    pub typ: TokenType,
    pub lexeme: String,
}

#[derive(Debug)]
#[derive(PartialEq)]
pub enum TokenType {
    LeftParen,
    RightParen,
    Equal,

    BangEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,

    Identifier,
    String(String),
    Int(i32),
    Float(f32),

    And,
    False,
    Null,
    Or,
    True,

    EOF
}

#[derive(Debug, PartialEq, Clone)]
pub enum Literal {
    False,
    True,
    Null,
    Int(i32),
    Float(f32),
    String(String)
}

#[derive(Debug, PartialEq)]
pub enum ComparisonOperator {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual
}

#[derive(Debug, Eq, PartialEq)]
pub enum LogicalOperator {
    And,
    Or
}

pub fn parse(str: String) -> Query {
    let res: Vec<&str> = str.rsplitn(3, " : ").collect();
    let mut it = res.iter().rev();

    let search;
    if let Some(s) = it.next() {
        search = s.replace(" \\: ", " : ");
    } else { search = str.clone(); }

    let mut expr = None;
    let mut lexing_errs = None;
    let mut parsing_errs = None;
    if let Some(expr_str) = it.next() {
        if !expr_str.is_empty() {
            let res = scan(expr_str);
            match res {
                Ok(t) => {
                    match Parser::parse(t) {
                        Ok(tree) => { expr = Some(tree); }
                        Err(err) => {
                            parsing_errs = Some(vec![err]);
                        }
                    }
                }
                Err(errs) => {
                    lexing_errs = Some(errs);
                }
            }
        }
    }

    let sort_target;
    if let Some(sort_str) = it.next() {
        match parse_sort_target(sort_str) {
            Ok(t) => { sort_target = t; }
            Err(err) => {
                if let Some(errs) = &mut parsing_errs {
                    errs.push(err);
                } else { parsing_errs = Some(vec![err]); }
                sort_target = None;
            }
        }
    } else { sort_target = None; }


    Query {
        search,
        expr,
        sort_target,
        lexing_errs,
        parsing_errs
    }
}

struct Parser {
    tokens: Vec<Token>,
    current: usize
}

#[derive(Debug)]
pub struct ParseError {
    pub message: String
}

impl Into<TranspilationError> for &ParseError {
    fn into(self) -> TranspilationError {
        TranspilationError {
            source: ErrorSource::Parsing,
            message: self.message.clone(),
        }
    }
}

impl Parser {
    pub fn parse(tokens: Vec<Token>) -> Result<Expr, ParseError> {
        let mut d = Parser {
            tokens,
            current: 0,
        };
        d.expression()
    }

    fn expression(&mut self) -> Result<Expr, ParseError> {
        let expr = self.equality()?;
        Ok(expr)
    }

    fn equality(&mut self) -> Result<Expr, ParseError> {
        self.logic_or()
    }

    fn comparison(&mut self) -> Result<Expr, ParseError> {
        return if self.peek().typ == TokenType::Identifier {
            self.binary_comparison()
        } else {
            self.ternary_comparison()
        }
    }

    fn binary_comparison(&mut self) -> Result<Expr, ParseError> {
        let id = self.consume(TokenType::Identifier, "Unexpected token in binary comparison. Expected identifier.")?.lexeme.clone();
        let op = self.operator()?;
        let lit = self.literal()?;
        Ok(Expr::Binary(Box::new(BinaryExpr {
            identifier: id,
            operator: op,
            literal: lit
        })))
    }

    fn ternary_comparison(&mut self) -> Result<Expr, ParseError> {
        let lit1 = self.literal()?;
        let op1 = self.operator()?;
        let id = self.consume(TokenType::Identifier, "Unexpected token in ternary comparison. Expected identifier.")?.lexeme.clone();
        let op2 = self.operator()?;
        let lit2 = self.literal()?;

        Ok(Expr::Ternary(Box::new(TernaryExpr {
            left_literal: lit1,
            left_operator: op1,
            identifier: id,
            right_operator: op2,
            right_literal: lit2,
        })))
    }

    fn grouping(&mut self) -> Result<Expr, ParseError> {
        return if self.match_this(TokenType::LeftParen) {
            let expr = self.logic_or();
            self.consume(TokenType::RightParen, "Expected ')'")?;
            expr
        } else {
            self.comparison()
        }
    }

    fn logic_or(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.logic_and()?;
        while self.match_this(TokenType::Or) {
            let right = self.logic_and()?;
            expr = Expr::Logical(Box::new(LogicalExpr { left: expr, operator: LogicalOperator::Or, right }));
        }

        Ok(expr)
    }

    fn logic_and(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.grouping()?;
        while self.match_this(TokenType::And) {
            let right = self.equality()?;
            expr = Expr::Logical(Box::new(LogicalExpr { left: expr, operator: LogicalOperator::And, right }));
        }
        Ok(expr)
    }

    fn operator(&mut self) -> Result<ComparisonOperator, ParseError> {
        if self.at_end() { return Err(self.error(&Token { typ: TokenType::EOF, lexeme: "EOF".to_string() }, "Unexpected end of expression")) }
        let token = self.peek();
        let op = match token.typ {
            TokenType::Equal => { ComparisonOperator::Equal }
            TokenType::BangEqual => { ComparisonOperator::NotEqual }
            TokenType::Greater => { ComparisonOperator::Greater }
            TokenType::GreaterEqual => { ComparisonOperator::GreaterEqual }
            TokenType::Less => { ComparisonOperator::Less }
            TokenType::LessEqual => { ComparisonOperator::LessEqual }
            _ => {
                return Err(self.error(token, "Unexpected token. Operator expected."))
            }
        };
        self.advance();
        Ok(op)
    }

    fn literal(&mut self) -> Result<Literal, ParseError> {
        if self.at_end() { return Err(self.error(&Token { typ: TokenType::EOF, lexeme: "EOF".to_string() }, "Unexpected end of expression")) }
        let token = self.peek();
        let op = match &token.typ {
            TokenType::True => Literal::True,
            TokenType::False => Literal::False,
            TokenType::Null => Literal::Null,
            TokenType::String(s) => Literal::String(s.clone()),
            TokenType::Int(i) => Literal::Int(i.clone()),
            TokenType::Float(f) => Literal::Float(f.clone()),
            _ => {
                return Err(self.error(token, "Unexpected token. Literal expected."))
            }
        };
        self.advance();
        Ok(op)
    }


    fn match_this(&mut self, typ: TokenType) -> bool {
        if self.check(typ) {
            self.advance();
            return true
        }

        false
    }

    fn check(&self, typ: TokenType) -> bool {
        if self.at_end() { return false }
        self.peek().typ == typ
    }


    fn advance(&mut self) -> &Token {
        if !self.at_end() { self.current += 1; }
        self.previous()
    }

    fn at_end(&self) -> bool {
        self.peek().typ == TokenType::EOF
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.current - 1]
    }

    fn consume(&mut self, typ: TokenType, message: &str) -> Result<&Token, ParseError> {
        if self.check(typ) { return Ok(self.advance()) }
        Err(self.error(self.peek(), message))
    }

    fn error(&self, token: &Token, message: &str) -> ParseError {
        if token.typ == TokenType::EOF {
            ParseError { message: format!("At end: {}", message) }
        } else {
            ParseError { message: format!("At '{}': {}", token.lexeme, message)}
        }
    }
}

fn parse_sort_target(str: &str) -> Result<Option<SortTarget>, ParseError> {
    let mut res = str.splitn(2, ' ');
    let identifier;
    if let Some(id) = res.next() {
        identifier = id.trim().to_string();
    } else {
        return Ok(None)
    }

    let direction;
    if let Some(dir) = res.next() {
        direction = Some(match dir.trim().to_lowercase().as_str() {
            "asc" => SortDirection::Ascending,
            "ascending" => SortDirection::Ascending,
            "desc" => SortDirection::Descending,
            "descending" => SortDirection::Descending,
            d => return Err(ParseError { message: format!("Invalid sorting direction '{}'", d) })
        })
    } else {
        direction = None
    }

    Ok(Some(SortTarget {
        identifier,
        direction,
    }))
}