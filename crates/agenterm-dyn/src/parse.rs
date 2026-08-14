use crate::error::DynError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SExpr {
    Int(i64),
    Str(String),
    Sym(String),
    List(Vec<SExpr>),
}

pub(crate) fn parse(source: &str) -> Result<SExpr, DynError> {
    let mut parser = Parser::new(source);
    let expr = parser.parse_expr()?;
    parser.skip_ws();
    if !parser.is_eof() {
        return Err(DynError::Parse("trailing tokens after expression".into()));
    }
    Ok(expr)
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn parse_expr(&mut self) -> Result<SExpr, DynError> {
        self.skip_ws();
        match self.peek() {
            Some('(') => self.parse_list(),
            Some('"') => self.parse_string(),
            Some('-') if self.peek_next_is_digit() => self.parse_int(),
            Some('-') => Ok(SExpr::Sym(self.parse_symbol()?)),
            Some('0'..='9') => self.parse_int(),
            Some(ch) if is_sym_start(ch) => Ok(SExpr::Sym(self.parse_symbol()?)),
            Some(ch) => Err(DynError::Parse(format!("unexpected character `{ch}`"))),
            None => Err(DynError::Parse("unexpected end of input".into())),
        }
    }

    fn peek_next_is_digit(&self) -> bool {
        let mut iter = self.input[self.pos..].chars();
        iter.next(); // skip leading '-'
        matches!(iter.next(), Some('0'..='9'))
    }

    fn parse_list(&mut self) -> Result<SExpr, DynError> {
        self.expect('(')?;
        self.skip_ws();
        let mut items = Vec::new();
        while self.peek() != Some(')') {
            if self.is_eof() {
                return Err(DynError::Parse("unclosed list".into()));
            }
            items.push(self.parse_expr()?);
            self.skip_ws();
        }
        self.expect(')')?;
        Ok(SExpr::List(items))
    }

    fn parse_string(&mut self) -> Result<SExpr, DynError> {
        self.expect('"')?;
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch == '"' {
                let s = self.input[start..self.pos].to_owned();
                self.bump();
                return Ok(SExpr::Str(s));
            }
            if ch == '\\' {
                return Err(DynError::Parse(
                    "escape sequences in strings are not supported yet".into(),
                ));
            }
            self.bump();
        }
        Err(DynError::Parse("unclosed string".into()))
    }

    fn parse_int(&mut self) -> Result<SExpr, DynError> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.bump();
        }
        if self.peek() == Some('0')
            && matches!(self.input[self.pos + 1..].chars().next(), Some('x' | 'X'))
        {
            return Err(DynError::Parse("hex integers are not supported".into()));
        }
        while matches!(self.peek(), Some('0'..='9')) {
            self.bump();
        }
        let slice = &self.input[start..self.pos];
        let n: i64 = slice
            .parse()
            .map_err(|_| DynError::Parse(format!("invalid integer `{slice}`")))?;
        Ok(SExpr::Int(n))
    }

    fn parse_symbol(&mut self) -> Result<String, DynError> {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if is_sym_char(ch) {
                self.bump();
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(DynError::Parse("empty symbol".into()));
        }
        Ok(self.input[start..self.pos].to_owned())
    }

    fn expect(&mut self, ch: char) -> Result<(), DynError> {
        if self.bump() == Some(ch) {
            Ok(())
        } else {
            Err(DynError::Parse(format!("expected `{ch}`")))
        }
    }
}

fn is_sym_start(ch: char) -> bool {
    matches!(ch, 'a'..='z' | 'A'..='Z' | '_' | '?' | '!' | '*' | '+' | '-' | '/' | '=' | '<' | '>')
}

fn is_sym_char(ch: char) -> bool {
    is_sym_start(ch) || matches!(ch, '0'..='9' | '.' | '@' | '#' | '%' | '^' | '&' | '$')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_list() {
        let expr = parse("(do (set x 1) x)").expect("parse");
        assert_eq!(
            expr,
            SExpr::List(vec![
                SExpr::Sym("do".into()),
                SExpr::List(vec![
                    SExpr::Sym("set".into()),
                    SExpr::Sym("x".into()),
                    SExpr::Int(1),
                ]),
                SExpr::Sym("x".into()),
            ])
        );
    }

    #[test]
    fn parses_minus_as_symbol_in_list() {
        let expr = parse("(- 5)").expect("parse unary sub form");
        assert_eq!(
            expr,
            SExpr::List(vec![SExpr::Sym("-".into()), SExpr::Int(5)])
        );
    }

    #[test]
    fn parses_negative_integer_literal() {
        assert_eq!(parse("-12").expect("negative literal"), SExpr::Int(-12));
    }
}
