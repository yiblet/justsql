use std::ops::{Deref, DerefMut};

use super::result::{PResult, ParseError};
use nom::Parser;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
/// maintains a suffix-based reference to where the value was parsed from
/// from the original &'a str.  useful for error reporting.
pub struct SpanRef<'a, A> {
    // the start of the suffix containing the value
    pub start: &'a str,
    // the end of the suffix containing the value
    pub end: &'a str,
    pub value: A,
}

impl<'a, A> SpanRef<'a, A> {
    /// the str containing the value
    pub fn value_str(&self) -> &'a str {
        &self.start[0..self.start.len() - self.end.len()]
    }

    pub fn map<B, F: FnOnce(A) -> B>(self, func: F) -> SpanRef<'a, B> {
        SpanRef {
            start: self.start,
            end: self.end,
            value: func(self.value),
        }
    }

    pub fn as_ref<'b>(&'b self) -> SpanRef<'a, &'b A> {
        SpanRef {
            start: self.start,
            end: self.end,
            value: &self.value,
        }
    }

    pub fn with<B>(&self, value: B) -> SpanRef<'a, B> {
        SpanRef {
            start: self.start,
            end: self.end,
            value,
        }
    }

    #[allow(dead_code)]
    pub fn as_ref_mut<'b>(&'b mut self) -> SpanRef<'a, &'b mut A> {
        SpanRef {
            start: self.start,
            end: self.end,
            value: &mut self.value,
        }
    }

    // maintain a reference of the original position
    pub fn parse<P>(mut parser: P) -> impl FnMut(&'a str) -> PResult<SpanRef<'a, A>>
    where
        P: Parser<&'a str, A, ParseError<'a>>,
    {
        move |input: &'a str| {
            let (output, res) = parser.parse(input)?;
            Ok((
                output,
                SpanRef {
                    start: input,
                    end: output,
                    value: res,
                },
            ))
        }
    }
}

impl<'a, A, E> SpanRef<'a, Result<A, E>> {
    #[allow(dead_code)]
    pub fn transpose(self) -> Result<SpanRef<'a, A>, E> {
        let reference = SpanRef {
            start: self.start,
            end: self.end,
            value: self.value?,
        };
        Ok(reference)
    }
}

impl<'a, A> SpanRef<'a, Option<A>> {
    #[allow(dead_code)]
    pub fn transpose(self) -> Option<SpanRef<'a, A>> {
        let reference = SpanRef {
            start: self.start,
            end: self.end,
            value: self.value?,
        };
        Some(reference)
    }
}

impl<'a, A> Deref for SpanRef<'a, A> {
    type Target = A;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<'a, A> DerefMut for SpanRef<'a, A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}
