fn main() {
    println!("{}", add(1, 2));
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::add;
    use rstest::rstest;
    use test_case::test_case;

    #[test]
    fn it_works() {
        assert_eq!(add(2, 2), 4);
    }

    #[rstest]
    #[case(1, 1, 2)]
    #[case::negative(-1, -1, -3)]
    fn sums(#[case] a: i32, #[case] b: i32, #[case] want: i32) {
        assert_eq!(add(a, b), want);
    }

    #[test_case(1, 2 => 3 ; "small")]
    #[test_case(2, 2 => 5 ; "wrong")]
    fn adds(a: i32, b: i32) -> i32 {
        add(a, b)
    }
}
