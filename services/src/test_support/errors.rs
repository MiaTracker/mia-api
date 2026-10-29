use crate::infrastructure::{RuleViolation, SrvErr};

/// Unwraps the rule violations from a failed service call; panics on success or any other error.
pub fn violations<T>(result: Result<T, SrvErr>) -> Vec<RuleViolation> {
    match result {
        Err(SrvErr::RuleViolation(violations)) => violations,
        Err(_) => panic!("expected a rule violation, got a different error"),
        Ok(_) => panic!("expected a rule violation, got Ok"),
    }
}

/// Asserts that the call failed with exactly one rule violation, and that `is_expected` accepts it.
pub fn assert_single_violation<T>(result: Result<T, SrvErr>, is_expected: fn(&RuleViolation) -> bool) {
    let violations = violations(result);
    assert_eq!(violations.len(), 1, "unexpected violations: {violations:?}");
    assert!(is_expected(&violations[0]), "unexpected violation: {:?}", violations[0]);
}
