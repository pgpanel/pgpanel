use pgpanel_core::validation::*;

#[test]
fn rejects_uppercase_and_symbols() {
    assert!(validate_safe_name("MyDB", "n").is_err());
    assert!(validate_safe_name("my-db", "n").is_err());
    assert!(validate_safe_name("my.db", "n").is_err());
}

#[test]
fn accepts_boundary_lengths() {
    assert!(validate_safe_name("abc", "n").is_ok());
    let long = format!("a{}", "b".repeat(62));
    assert_eq!(long.len(), 63);
    assert!(validate_safe_name(&long, "n").is_ok());
    let too_long = format!("a{}", "b".repeat(63));
    assert!(validate_safe_name(&too_long, "n").is_err());
}

#[test]
fn resource_limits() {
    assert!(validate_cpu_limit(0.05).is_err());
    assert!(validate_cpu_limit(2.0).is_ok());
    assert!(validate_memory_mb(64).is_err());
    assert!(validate_memory_mb(512).is_ok());
    assert!(validate_public_port(80).is_err());
    assert!(validate_public_port(5433).is_ok());
}
