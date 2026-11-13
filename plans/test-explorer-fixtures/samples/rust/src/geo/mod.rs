pub fn dist(a: f64, b: f64) -> f64 {
    (a - b).abs()
}

#[cfg(test)]
mod tests {
    #[test]
    fn dist_is_symmetric() {
        assert_eq!(super::dist(1.0, 3.0), super::dist(3.0, 1.0));
    }

    #[test]
    fn unwraps_none() {
        let x: Option<u8> = None;
        x.unwrap();
    }
}
