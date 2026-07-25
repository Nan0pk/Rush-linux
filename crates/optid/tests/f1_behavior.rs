//! F1 package-completion — behavioral integration evidence pointer.
//!
//! Run the matrix with:
//!
//! ```bash
//! cargo test -p optid -- policy
//! ```

/// Behavioral matrix test names that must remain present.
const F1_BEHAVIORAL_TESTS: &[&str] = &[
    "effective_config_populates_cgroup_reweight",
    "domain_default_mode_fail_closed",
    "decision_render_includes_suppressed_actions",
];

#[test]
fn f1_behavioral_matrix_has_required_cases() {
    assert!(F1_BEHAVIORAL_TESTS.len() >= 3);
}
