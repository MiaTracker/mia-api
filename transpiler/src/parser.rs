use crate::{ErrorSource, TranspilationError};
use crate::lexer::{LexingError, scan};

pub struct Query {
    pub search: String,
    pub expr: Option<Expr>,
    pub lexing_errs: Option<Vec<LexingError>>,
    pub parsing_err: Option<ParseError>,
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

#[derive(Debug, PartialEq)]
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

#[derive(Debug)]
pub enum LogicalOperator {
    And,
    Or
}

pub fn parse(str: String) -> Query {
    let res = str.rsplit_once(" : ");
    let search;
    let mut expr = None;
    let mut lexing_errs = None;
    let mut parsing_err = None;
    if let Some((src, expr_str)) = res {
        search = src.to_string();
        let res = scan(expr_str);
        match res {
            Ok(t) => {
                match Parser::parse(t) {
                    Ok(tree) => { expr = Some(tree); }
                    Err(err) => {
                        parsing_err = Some(err);
                    }
                }
            }
            Err(errs) => {
                lexing_errs = Some(errs);
            }
        }
    } else { search = str }

    let search = search.replace(" \\: ", " : ");

    Query {
        search,
        expr,
        lexing_errs,
        parsing_err
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

impl Into<TranspilationError> for ParseError {
    fn into(self) -> TranspilationError {
        TranspilationError {
            source: ErrorSource::Parsing,
            message: self.message,
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