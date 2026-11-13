#[test]
fn before_the_crash() {}

#[test]
fn aborts() {
    std::thread::sleep(std::time::Duration::from_millis(50));
    std::process::abort();
}
