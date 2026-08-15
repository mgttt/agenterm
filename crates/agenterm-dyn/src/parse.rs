use crate::error::DynError;

/// Maximum number of nested S-expression lists accepted by the parser.
///
/// Keeping this bounded prevents adversarial input from exhausting the Rust
/// call stack before evaluation can apply its own limits.
pub(crate) const MAX_LIST_DEPTH: usize = 256;

/// Maximum UTF-8 source size accepted before parsing begins.
pub(crate) const MAX_SOURCE_BYTES: usize = 64 * 1024;

/// Maximum AST nodes accepted by one source expression.
///
/// Lists and scalar expressions each count as one node. Keeping this bounded
/// prevents a shallow but extremely wide expression from consuming unbounded
/// parser allocation even when it stays within the nesting limit.
pub(crate) const MAX_AST_NODES: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SExpr {
    Int(i64),
    Str(String),
    Sym(String),
    List(Vec<SExpr>),
}

pub(crate) fn parse(source: &str) -> Result<SExpr, DynError> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(DynError::Parse(format!(
            "source exceeds maximum size ({MAX_SOURCE_BYTES} bytes)"
        )));
    }
    let mut parser = Parser::new(source);
    let expr = parser.parse_expr(0)?;
    parser.skip_ws();
    if !parser.is_eof() {
        return Err(DynError::Parse("trailing tokens after expression".into()));
    }
    Ok(expr)
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
    ast_nodes: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            ast_nodes: 0,
        }
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

    fn parse_expr(&mut self, list_depth: usize) -> Result<SExpr, DynError> {
        self.skip_ws();
        match self.peek() {
            Some('(') => {
                self.reserve_ast_node()?;
                self.parse_list(list_depth)
            }
            Some('"') => {
                self.reserve_ast_node()?;
                self.parse_string()
            }
            Some('-') if self.peek_next_is_digit() => {
                self.reserve_ast_node()?;
                self.parse_int()
            }
            Some('-') => {
                self.reserve_ast_node()?;
                Ok(SExpr::Sym(self.parse_symbol()?))
            }
            Some('0'..='9') => {
                self.reserve_ast_node()?;
                self.parse_int()
            }
            Some(ch) if is_sym_start(ch) => {
                self.reserve_ast_node()?;
                Ok(SExpr::Sym(self.parse_symbol()?))
            }
            Some(ch) => Err(DynError::Parse(format!("unexpected character `{ch}`"))),
            None => Err(DynError::Parse("unexpected end of input".into())),
        }
    }

    fn reserve_ast_node(&mut self) -> Result<(), DynError> {
        if self.ast_nodes >= MAX_AST_NODES {
            return Err(DynError::Parse(format!(
                "maximum AST node count ({MAX_AST_NODES}) exceeded"
            )));
        }
        self.ast_nodes += 1;
        Ok(())
    }

    fn peek_next_is_digit(&self) -> bool {
        let mut iter = self.input[self.pos..].chars();
        iter.next(); // skip leading '-'
        matches!(iter.next(), Some('0'..='9'))
    }

    fn parse_list(&mut self, list_depth: usize) -> Result<SExpr, DynError> {
        if list_depth >= MAX_LIST_DEPTH {
            return Err(DynError::Parse(format!(
                "maximum list nesting depth ({MAX_LIST_DEPTH}) exceeded"
            )));
        }
        self.expect('(')?;
        self.skip_ws();
        let mut items = Vec::new();
        while self.peek() != Some(')') {
            if self.is_eof() {
                return Err(DynError::Parse("unclosed list".into()));
            }
            items.push(self.parse_expr(list_depth + 1)?);
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

    fn nested_list_source(depth: usize) -> String {
        format!("{}0{}", "(".repeat(depth), ")".repeat(depth))
    }

    fn flat_list_source(item_count: usize) -> String {
        format!("({})", "1 ".repeat(item_count))
    }

    #[test]
    fn accepts_maximum_list_nesting_depth() {
        parse(&nested_list_source(MAX_LIST_DEPTH)).expect("maximum nesting parses");
    }

    #[test]
    fn rejects_list_nesting_beyond_maximum_depth() {
        assert_eq!(
            parse(&nested_list_source(MAX_LIST_DEPTH + 1)),
            Err(DynError::Parse(format!(
                "maximum list nesting depth ({MAX_LIST_DEPTH}) exceeded"
            )))
        );
    }

    #[test]
    fn accepts_maximum_source_bytes() {
        parse(&format!("1{}", " ".repeat(MAX_SOURCE_BYTES - 1)))
            .expect("maximum source size parses");
    }

    #[test]
    fn rejects_source_bytes_beyond_maximum() {
        assert_eq!(
            parse(&format!("1{}", " ".repeat(MAX_SOURCE_BYTES))),
            Err(DynError::Parse(format!(
                "source exceeds maximum size ({MAX_SOURCE_BYTES} bytes)"
            )))
        );
    }

    #[test]
    fn accepts_maximum_ast_nodes() {
        // The list is one node; its scalar items consume the remaining budget.
        parse(&flat_list_source(MAX_AST_NODES - 1)).expect("maximum AST nodes parse");
    }

    #[test]
    fn rejects_ast_nodes_beyond_maximum() {
        assert_eq!(
            parse(&flat_list_source(MAX_AST_NODES)),
            Err(DynError::Parse(format!(
                "maximum AST node count ({MAX_AST_NODES}) exceeded"
            )))
        );
    }
}
