use super::*;

#[test]
fn composer_wire_tokens_accept_only_canonical_nonzero_decimal_u64() {
    let (generation, lease) =
        parse_composer_wire_tokens("7", "9").expect("canonical composer tokens");
    assert_eq!(generation.to_wire_string(), "7");
    assert_eq!(lease.to_wire_string(), "9");

    for (generation, lease) in [("0", "1"), ("01", "1"), ("1", "00")] {
        assert!(
            parse_composer_wire_tokens(generation, lease).is_err(),
            "noncanonical composer token pair must be rejected"
        );
    }
    assert!(parse_composer_wire_tokens("18446744073709551616", "1").is_err());
}
