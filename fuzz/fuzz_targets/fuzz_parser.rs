#![no_main]
use libfuzzer_sys::fuzz_target;
use calvin_core::context::TypeContext;
use calvin_parse::lexer::Token;
use chumsky::input::Input;
use chumsky::Parser;
use chumsky::input::Stream;
use logos::Logos;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let ctx = TypeContext::new();
        let src: &str = ctx.arena().alloc_str(s);
        
        let token_iter = Token::lexer(src)
            .spanned()
            .map(|(tok, span)| match tok {
                Ok(t) => Ok((t, span)),
                Err(e) => Err((e, span)),
            });
            
        let mut tokens = Vec::new();
        for t in token_iter {
            if let Ok(t) = t { tokens.push(t); } else { return; }
        }
        
        let eof = src.len()..src.len();
        let token_stream = Stream::from_iter(tokens.into_iter()).map(eof, |(t, s)| (t, s));

        let _ = calvin_parse::parser::expr_parser(&ctx).parse(token_stream).into_result();
    }
});
