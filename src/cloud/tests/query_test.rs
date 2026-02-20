use crate::cloud::common::get_ppid;

#[test]
fn test_get_ppid_returns_some() {
    let ppid = get_ppid();
    assert!(
        ppid.is_some(),
        "get_ppid() should return Some on Unix/Windows"
    );
    assert!(ppid.is_some_and(|p| p > 0), "ppid should be > 0");
}
