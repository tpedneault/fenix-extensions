//! A tiny crate with tests of every kind the explorer must show.
pub mod geo;

/// Adds.
///
/// ```
/// assert_eq!(orbit::add(2, 2), 4);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds() {
        assert_eq!(add(1, "2"), 3);
    }

    #[test]
    fn fails_on_purpose() {
        println!("some output before the failure");
        assert_eq!(add(2, 2), 5, "two and two");
    }

    #[test]
    #[ignore = "slow"]
    fn ignored_one() {}

    #[test]
    #[should_panic(expected = "boom")]
    fn panics_as_it_should() {
        panic!("boom");
    }

    macro_rules! cases {
        ($($name:ident: $a:expr, $b:expr, $want:expr;)*) => {
            $(#[test] fn $name() { assert_eq!(add($a, $b), $want); })*
        };
    }
    cases! {
        case_small: 1, 1, 2;
        case_negative: -1, -1, -3;
    }

    mod nested {
        #[test]
        fn deep() {}
    }
}
