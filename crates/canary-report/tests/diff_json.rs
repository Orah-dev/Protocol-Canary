//! `diffVersion: 1` diff JSON serialization
//! (`cargo test -p canary-report --test diff_json`), covering issue
//! #449: deterministic, CF-05-conforming rendering of a report
//! comparison.

use canary_core::{
    CompatibilityResult, GitContext, PolicyDecision, ProjectType, ProtocolVersion, Status, Surface,
};
use canary_report::{
    compare, DiffJsonReporter, DiffReport, DiffVerdict, ProjectSummary, ReportInput,
};

fn result(surface: Surface, test_id: &str, status: Status) -> CompatibilityResult {
    CompatibilityResult {
        test_id: test_id.to_string(),
        protocol: ProtocolVersion(28),
        surface,
        status,
        summary: "synthetic".into(),
        details: None,
        duration_ms: 1,
        fixture_id: Some(test_id.to_string()),
    }
}

fn report(results: Vec<CompatibilityResult>) -> ReportInput {
    ReportInput {
        tool_version: "0.0.0-test".into(),
        target_protocol: ProtocolVersion(28),
        project: ProjectSummary {
            name: "diff-json".into(),
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

fn diff(baseline: ReportInput, current: ReportInput) -> DiffReport {
    compare(&baseline, &current)
}

#[test]
fn renders_an_integer_diff_version_and_a_machine_readable_verdict() {
    let d = diff(
        report(vec![result(Surface::Xdr, "t1", Status::Pass)]),
        report(vec![result(Surface::Xdr, "t1", Status::Fail)]),
    );
    let value: serde_json::Value =
        serde_json::from_str(&DiffJsonReporter::render(&d)).expect("renders valid JSON");

    assert_eq!(value["diffVersion"], 1, "diffVersion must be the integer 1");
    assert!(value["diffVersion"].is_number());
    assert_eq!(value["verdict"], "regression");
    assert!(value["transitions"].is_array());
    assert!(value["missing"].is_array());

    let transition = &value["transitions"][0];
    assert_eq!(transition["surface"], "xdr");
    assert_eq!(transition["testId"], "t1");
    assert_eq!(transition["from"], "pass");
    assert_eq!(transition["to"], "fail");
}

#[test]
fn transitions_sort_by_surface_alphabetically_then_test_id() {
    // The same test id on all three surfaces plus two ids on one
    // surface: surfaces must come out rpc < soroban < xdr, ids
    // lexicographically within a surface.
    let baseline = report(vec![
        result(Surface::Xdr, "t1", Status::Pass),
        result(Surface::Rpc, "t1", Status::Pass),
        result(Surface::Soroban, "t1", Status::Pass),
        result(Surface::Xdr, "a-first", Status::Pass),
        result(Surface::Xdr, "z-last", Status::Pass),
    ]);
    let current = report(vec![
        result(Surface::Xdr, "t1", Status::Fail),
        result(Surface::Rpc, "t1", Status::Fail),
        result(Surface::Soroban, "t1", Status::Fail),
        result(Surface::Xdr, "a-first", Status::Fail),
        result(Surface::Xdr, "z-last", Status::Fail),
    ]);

    let value: serde_json::Value =
        serde_json::from_str(&DiffJsonReporter::render(&diff(baseline, current))).expect("json");

    let order: Vec<(String, String)> = value["transitions"]
        .as_array()
        .expect("transitions array")
        .iter()
        .map(|t| {
            (
                t["surface"].as_str().unwrap().to_string(),
                t["testId"].as_str().unwrap().to_string(),
            )
        })
        .collect();

    assert_eq!(
        order,
        vec![
            ("rpc".to_string(), "t1".to_string()),
            ("soroban".to_string(), "t1".to_string()),
            ("xdr".to_string(), "a-first".to_string()),
            ("xdr".to_string(), "t1".to_string()),
            ("xdr".to_string(), "z-last".to_string()),
        ],
        "identical ids across surfaces must sort surface-alphabetically"
    );
}

#[test]
fn missing_entries_are_listed_with_their_baseline_status() {
    let baseline = report(vec![
        result(Surface::Rpc, "gone-rpc", Status::Pass),
        result(Surface::Xdr, "gone-xdr", Status::Fail),
    ]);
    let value: serde_json::Value =
        serde_json::from_str(&DiffJsonReporter::render(&diff(baseline, report(Vec::new()))))
            .expect("json");

    let missing: Vec<(String, String, String)> = value["missing"]
        .as_array()
        .expect("missing array")
        .iter()
        .map(|m| {
            (
                m["surface"].as_str().unwrap().to_string(),
                m["testId"].as_str().unwrap().to_string(),
                m["baselineStatus"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        missing,
        vec![
            ("rpc".to_string(), "gone-rpc".to_string(), "pass".to_string()),
            ("xdr".to_string(), "gone-xdr".to_string(), "fail".to_string()),
        ]
    );
    assert_eq!(value["verdict"], "coverage-lost");
}

#[test]
fn the_added_field_is_omitted_entirely_when_there_is_none() {
    let unchanged = report(vec![result(Surface::Xdr, "t1", Status::Pass)]);
    let value: serde_json::Value =
        serde_json::from_str(&DiffJsonReporter::render(&diff(unchanged.clone(), unchanged)))
            .expect("json");
    assert!(
        value.get("added").is_none(),
        "`added` must be absent rather than empty, got: {value}"
    );
}

#[test]
fn added_entries_appear_when_the_current_report_covers_new_tests() {
    let baseline = report(vec![result(Surface::Xdr, "t1", Status::Pass)]);
    let current = report(vec![
        result(Surface::Xdr, "t1", Status::Pass),
        result(Surface::Soroban, "new-test", Status::Pass),
    ]);
    let value: serde_json::Value =
        serde_json::from_str(&DiffJsonReporter::render(&diff(baseline, current))).expect("json");

    assert_eq!(value["added"][0]["surface"], "soroban");
    assert_eq!(value["added"][0]["testId"], "new-test");
    assert_eq!(value["added"][0]["status"], "pass");
    // New coverage is informational: it never changes the verdict.
    assert_eq!(value["verdict"], "no-change");
}

#[test]
fn verdicts_follow_the_documented_strict_precedence() {
    // A regression alongside both an improvement and lost coverage:
    // regression wins.
    let baseline = report(vec![
        result(Surface::Xdr, "worsens", Status::Pass),
        result(Surface::Xdr, "improves", Status::Fail),
        result(Surface::Xdr, "vanishes", Status::Pass),
    ]);
    let current = report(vec![
        result(Surface::Xdr, "worsens", Status::Fail),
        result(Surface::Xdr, "improves", Status::Pass),
    ]);
    let value: serde_json::Value =
        serde_json::from_str(&DiffJsonReporter::render(&diff(baseline, current))).expect("json");
    assert_eq!(value["verdict"], "regression");

    // Coverage loss beats improvement.
    let baseline = report(vec![
        result(Surface::Xdr, "improves", Status::Fail),
        result(Surface::Xdr, "vanishes", Status::Pass),
    ]);
    let current = report(vec![result(Surface::Xdr, "improves", Status::Pass)]);
    let value: serde_json::Value =
        serde_json::from_str(&DiffJsonReporter::render(&diff(baseline, current))).expect("json");
    assert_eq!(value["verdict"], "coverage-lost");

    // Improvement with no losses.
    let baseline = report(vec![result(Surface::Xdr, "improves", Status::Fail)]);
    let current = report(vec![result(Surface::Xdr, "improves", Status::Pass)]);
    let value: serde_json::Value =
        serde_json::from_str(&DiffJsonReporter::render(&diff(baseline, current))).expect("json");
    assert_eq!(value["verdict"], "improved");
}

#[test]
fn rendering_is_byte_deterministic() {
    let baseline = report(vec![
        result(Surface::Xdr, "t1", Status::Pass),
        result(Surface::Rpc, "t2", Status::Fail),
    ]);
    let current = report(vec![
        result(Surface::Xdr, "t1", Status::Fail),
        result(Surface::Soroban, "t3", Status::Pass),
    ]);
    // Two separate comparisons of the same pair — not one rendered
    // twice — so this also covers the entry-ordering step.
    assert_eq!(
        DiffJsonReporter::render(&diff(baseline.clone(), current.clone())),
        DiffJsonReporter::render(&diff(baseline, current))
    );
}

#[test]
fn rendered_output_parses_back_into_an_equivalent_diff() {
    let baseline = report(vec![
        result(Surface::Xdr, "t1", Status::Pass),
        result(Surface::Rpc, "gone", Status::Warning),
    ]);
    let current = report(vec![
        result(Surface::Xdr, "t1", Status::Fail),
        result(Surface::Soroban, "new", Status::Pass),
    ]);
    let original = diff(baseline, current);
    let parsed = DiffJsonReporter::parse(&DiffJsonReporter::render(&original)).expect("parses");
    assert_eq!(parsed, original);
}

#[test]
fn rejects_an_unrecognized_verdict() {
    let mut value: serde_json::Value =
        serde_json::from_str(&DiffJsonReporter::render(&diff(
            report(Vec::new()),
            report(Vec::new()),
        )))
        .expect("json");
    value["verdict"] = "catastrophe".into();
    let err = DiffJsonReporter::parse(&serde_json::to_string(&value).unwrap()).unwrap_err();
    assert!(err.to_string().contains("verdict"), "got: {err}");
}

#[test]
fn the_diff_version_field_survives_a_round_trip() {
    let d = diff(
        report(vec![result(Surface::Xdr, "t1", Status::Pass)]),
        report(vec![result(Surface::Xdr, "t1", Status::Fail)]),
    );
    let parsed = DiffJsonReporter::parse(&DiffJsonReporter::render(&d)).expect("parses");
    assert_eq!(parsed.diff_version, 1);
    assert_eq!(parsed.verdict, DiffVerdict::Regression);
}
