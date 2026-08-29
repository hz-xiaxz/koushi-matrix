//! RED contract tests for the #699 local QA scenario.

use super::registry::{QaScenario, final_tokens_for_scenario, scenario_report};

#[test]
fn e2ee_login_store_parses_with_exact_private_safe_tokens() {
    let scenario = QaScenario::E2eeLoginStore;

    assert_eq!(QaScenario::from_env_value("e2ee_login_store"), Ok(scenario));
    assert_eq!(
        final_tokens_for_scenario(scenario),
        [
            "safety=ok",
            "e2ee_login_store_fresh_offline_index0=ok",
            "e2ee_login_store_restore_offline_index0=ok",
            "e2ee_login_store_restart_offline_index0=ok",
            "e2ee_login_store_reauth_offline_index0=ok",
            "e2ee_login_store_online_index0=ok",
            "e2ee_login_store_group_index0=ok",
            "e2ee_login_store_identity_stable=ok",
            "e2ee_login_store=ok",
        ]
    );

    let report = scenario_report("local", scenario);
    assert!(!report.contains('@'));
    assert!(!report.contains('!'));
    assert!(!report.contains('$'));
}
