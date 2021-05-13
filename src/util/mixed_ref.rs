use std::{borrow::Borrow, ops::Deref};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MixedRef<'a, A> {
    Borrowed(&'a A),
    Owned(A),
}

impl<'a, A> Deref for MixedRef<'a, A> {
    type Target = A;

    fn deref(&self) -> &Self::Target {
        match self {
            MixedRef::Borrowed(b) => *b,
            MixedRef::Owned(b) => b,
        }
    }
}

impl<'a, A> Borrow<A> for MixedRef<'a, A> {
    fn borrow(&self) -> &A {
        self.deref()
    }
}
