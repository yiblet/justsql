#[macro_export]
macro_rules! matches_map {
    ($expression:expr, $( $pattern:pat )|+ $( if $guard: expr )? => $eval:expr ) => {
        match $expression {
            $( $pattern )|+ $( if $guard )? => Some ( $eval ),
            _ => None
        }
    }
}
