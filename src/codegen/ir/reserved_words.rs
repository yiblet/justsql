use crate::codegen::{
    result::{CResult, IrErrorKind, ParseError},
    span_ref::SpanRef,
};

const RESERVED_WORDS: [&'static str; 5] = ["auth", "import", "param", "throw", "endpoint"];

pub fn check_reserved_words<'b, 'a: 'b, I: Iterator<Item = SpanRef<'a, &'b str>> + 'b>(
    iter: I,
) -> impl Iterator<Item = ParseError<'a>> + 'b {
    iter.filter_map(|res: SpanRef<'a, &'b str>| {
        if RESERVED_WORDS.contains(&res.trim()) {
            Some(ParseError::IrErrorKind(
                res.start,
                IrErrorKind::ReservedWordError(res.trim().to_string()),
            ))
        } else {
            None
        }
    })
}
