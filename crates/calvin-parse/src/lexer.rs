use logos::Logos;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Default)]
pub enum LexicalError {
    #[default]
    InvalidToken,
    UnclosedString,
}

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(error = LexicalError)]
#[logos(skip r"[ \t\r\n\f]+")] // Skip spaces, tabs, newlines, form feeds.
#[logos(skip r"//[^\n]*")] // Skip line comments
#[logos(skip r"/\*([^*]|\*[^/])*\*/")] // Skip block comments
#[logos(skip r"\{-#([^-]|-+[^#-])*-+#\}")] // Skip pragmas like {-# UNSAFE ... #-}
pub enum Token<'a> {
    // Keywords
    #[token("let")]
    Let,
    #[token("in")]
    In,
    #[token("if")]
    If,
    #[token("then")]
    Then,
    #[token("else")]
    Else,
    #[token("fn")]
    Fn,
    #[token("match")]
    Match,
    #[token("with")]
    With,
    #[token("case")]
    Case,
    #[token("class")]
    Class,
    #[token("instance")]
    Instance,
    #[token("where")]
    Where,
    #[token("module")]
    Module,
    #[token("import")]
    Import,
    #[token("type")]
    Type,
    #[token("data")]
    Data,
    #[token("do")]
    Do,
    #[token("return")]
    Return,

    // Symbols
    #[token("=")]
    Eq,
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,
    #[token("::", priority = 20)]
    DoubleColon,
    #[token("\\")]
    Lambda,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token(";")]
    Semi,
    #[token(".")]
    Dot,
    #[token("|")]
    Pipe,
    #[token("_", priority = 10)]
    Underscore,

    // Operators
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("==")]
    DoubleEq,
    #[token("!=")]
    NotEq,
    #[token("<=", priority = 10)]
    Lte,
    #[token(">=", priority = 10)]
    Gte,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("===", priority = 15)]
    TripleEq,
    #[token("!==", priority = 15)]
    ExclDoubleEq,
    #[token("~")]
    Tilde,
    #[token("++", priority = 10)]
    PlusPlus,
    #[token("<-", priority = 10)]
    LeftArrow,
    #[token("and")]
    And,
    #[token("or")]
    Or,
    #[token("not")]
    Not,

    // Identifiers and Literals
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
    Ident(&'a str),

    #[regex(r"0[xX][0-9a-fA-F]+", |lex| {
        let s = &lex.slice()[2..];
        u8::from_str_radix(s, 16).unwrap_or(0)
    })]
    Byte(u8),

    #[regex(r"[0-9]+S", |lex| {
        let s = lex.slice();
        s[..s.len()-1].parse::<i16>().unwrap_or(0)
    })]
    Short(i16),

    #[regex(r"[0-9]+[lL]", |lex| {
        let s = lex.slice();
        s[..s.len()-1].parse::<i64>().unwrap_or(0)
    })]
    Long(i64),

    #[regex(r"[0-9]+[hH]", |lex| {
        let s = lex.slice();
        s[..s.len()-1].parse::<i128>().unwrap_or(0)
    })]
    Int128(i128),

    #[regex(r"[0-9]+s", |lex| {
        let s = lex.slice();
        s[..s.len()-1].parse::<i64>().unwrap_or(0)
    })]
    Timespan(i64),

    #[regex(r"[0-9]+\.[0-9]+[fF]", |lex| {
        let s = lex.slice();
        s[..s.len()-1].parse::<f64>().unwrap_or(0.0)
    })]
    Float(f64),

    #[regex(r"[0-9]+\.[0-9]+", |lex| lex.slice().parse::<f64>().unwrap_or(0.0))]
    Double(f64),

    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().unwrap_or(0))]
    Int(i64),

    #[regex(r"'([^'\\]|\\.)*'", |lex| {
        let s = lex.slice();
        if s.len() >= 3 {
            let inner = &s[1..s.len()-1];
            if inner.starts_with('\\') {
                match inner.chars().nth(1) {
                    Some('n') => '\n',
                    Some('t') => '\t',
                    Some('r') => '\r',
                    Some('\\') => '\\',
                    Some('\'') => '\'',
                    Some('0') => '\0',
                    Some(c) => c,
                    None => '\0',
                }
            } else {
                inner.chars().next().unwrap_or('\0')
            }
        } else {
            '\0'
        }
    })]
    Char(char),

    #[regex(r#""([^"\\]|\\.)*""#, |lex| &lex.slice()[1..lex.slice().len()-1])]
    String(&'a str),
}

impl<'a> fmt::Display for Token<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
