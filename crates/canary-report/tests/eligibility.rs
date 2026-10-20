//! CF-05 comparison eligibility (`cargo test -p canary-report --test
//! eligibility`), covering issue #447: refuse to compare reports whose
//! `schemaVersion` or `targetProtocol` differ, with an explanatory
//! diagnostic the CLI maps to exit code 2.

use canary_core::{
    CompatibilityResult, GitContext, PolicyDecision, ProjectType, ProtocolVersion, Status, Surface,
};
use canary_report::diff::{validate_eligibility, IncompatibleReportsError, StoredReport};
use canary_report::json::SCHEMA_VERSION;
use canary_report::{DiffVerdict, ProjectSummary, ReportInput};

fn result(test_id: &str, status: Status) -> CompatibilityResult {
    CompatibilityResult {
        test_id: test_id.to_string(),
        protocol: ProtocolVersion(28),
        surface: Surface::Xdr,
        status,
        summary: "synthetic".into(),
        details: None,
        duration_ms: 1,
        fixture_id: Some(test_id.to_string()),
    }
}

fn report(protocol: u32, results: Vec<CompatibilityResult>) -> ReportInput {
    ReportInput {
        tool_version: "0.0.0-test".into(),
        target_protocol: ProtocolVersion(protocol),
        project: ProjectSummary {
            name: "eligibility".into(),
            project_type: ProjectType::Unknown,
        },
        network: None,
        results,
        skipped: Vec::new(),
        decision: PolicyDecision::Pass,
        git: GitContext::default(),
        verbose: false,
    }
}

fn stored(schema_version: u32, input: ReportInput) -> StoredReport {
    StoredReport {
        schema_version,
        input,
    }
}

fn protocol_28() -> ReportInput {
    report(28, vec![result("p28-xdr-1", Status::Pass)])
}

#[test]
fn matching_schema_and_protocol_are_eligible() {
    validate_eligibility(
        &stored(SCHEMA_VERSION, protocol_28()),
        &stored(SCHEMA_VERSION, protocol_28()),
    )
    .expect("identically shaped reports must be comparable");
}

#[test]
fn differing_schema_versions_are_refused_with_a_clear_diagnostic() {
    let err = validate_eligibility(
        &stored(SCHEMA_VERSION, protocol_28()),
        &stored(SCHEMA_VERSION + 1, protocol_28()),
    )
    .expect_err("a schemaVersion mismatch must refuse comparison");

    assert_eq!(
        err,
        IncompatibleReportsError::SchemaVersionMismatch {
            baseline: SCHEMA_VERSION,
            current: SCHEMA_VERSION + 1,
        }
    );
    let message = err.to_string();
    assert!(
        message.contains("schemaVersion mismatch"),
        "diagnostic must name the mismatch, got: {message}"
    );
}

#[test]
fn comparing_protocol_28_against_protocol_29_is_refused() {
    let baseline = report(28, vec![result("p28-xdr-1", Status::Pass)]);
    let current = report(29, vec![result("p28-xdr-1", Status::Pass)]);

    let err = validate_eligibility(
        &stored(SCHEMA_VERSION, baseline),
        &stored(SCHEMA_VERSION, current),
    )
    .expect_err("cross-protocol comparison must be refused");

    assert_eq!(
        err,
        IncompatibleReportsError::ProtocolMismatch {
            baseline: 28,
            current: 29,
        }
    );
    let message = err.to_string();
    assert!(
        message.contains("protocol mismatch"),
        "diagnostic must name the protocol mismatch, got: {message}"
    );
    assert!(
        message.contains("28") && message.contains("29"),
        "diagnostic must name both protocols, got: {message}"
    );
}

#[test]
fn equal_but_unsupported_schema_versions_are_refused() {
    // Two reports written by some future schema are equally ineligible:
    // this build cannot interpret either of them faithfully.
    let err = validate_eligibility(&stored(999, protocol_28()), &stored(999, protocol_28()))
        .expect_err("unsupported schemaVersion must refuse comparison");

    assert!(matches!(
        err,
        IncompatibleReportsError::UnsupportedSchemaVersion {
            found: 999,
            supported
        } if supported == SCHEMA_VERSION
    ));
}

#[test]
fn schema_mismatch_wins_over_a_protocol_mismatch_in_the_diagnostic() {
    // Both differ: the schema check runs first, so the refusal names the
    // incompatibility that would break parsing before the one that
    // breaks meaning.
    let err = validate_eligibility(
        &stored(SCHEMA_VERSION, protocol_28()),
        &stored(SCHEMA_VERSION + 1, report(29, Vec::new())),
    )
    .expect_err("mismatched schemas refuse before protocols are considered");
    assert!(matches!(
        err,
        IncompatibleReportsError::SchemaVersionMismatch { .. }
    ));
}

#[test]
fn an_ineligible_comparison_produces_no_diff_at_all() {
    use canary_report::diff::compare_reports;

    let baseline = stored(SCHEMA_VERSION, report(28, vec![result("t", Status::Fail)]));
    let current = stored(SCHEMA_VERSION, report(29, vec![result("t", Status::Pass)]));

    // Without the eligibility gate this would look like a beautiful
    // "improvement" — which is exactly why it must never be produced.
    let err = compare_reports(baseline, current)
        .expect_err("compare_reports must gate on eligibility");
    assert!(matches!(
        err,
        IncompatibleReportsError::ProtocolMismatch { .. }
    ));
    // Sanity: the verdict type exists and the refusal never renders as one.
    assert!(!DiffVerdict::NoChange.is_failure());
}
