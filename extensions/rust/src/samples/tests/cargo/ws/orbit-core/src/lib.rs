/// Doubles.
///
/// ```
/// assert_eq!(orbit_core::double(2), 4);
/// ```
pub fn double(x: i32) -> i32 {
    x * 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(super::double(2), 4);
    }
}
