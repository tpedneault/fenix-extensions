#[test]
fn integration_adds() {
    assert_eq!(orbit::add(20, 22), 42);
}

#[test]
fn integration_fails() {
    assert!(orbit::add(0, 0) == 1, "zero is not one");
}
