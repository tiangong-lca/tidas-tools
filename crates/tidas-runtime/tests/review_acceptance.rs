//! Temporary review-service acceptance fixture; no production code uses it.
//! A budget ledger permits zero remaining bytes but rejects an overdrawn state.

fn remaining_budget(limit: u64, used: u64) -> Option<u64> {
    Some(limit.saturating_sub(used))
}

#[test]
fn ordinary_remaining_budget() {
    assert_eq!(remaining_budget(100, 40), Some(60));
}

#[test]
fn exactly_exhausted_budget_is_valid() {
    assert_eq!(remaining_budget(100, 100), Some(0));
}

#[test]
fn overdrawn_budget_is_rejected() {
    assert_eq!(remaining_budget(100, 101), None);
}

#[test]
fn maximum_counter_does_not_hide_overdraw() {
    assert_eq!(remaining_budget(1, u64::MAX), None);
}
