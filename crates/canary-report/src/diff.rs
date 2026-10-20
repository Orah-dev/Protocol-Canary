//! Comparing two stored reports (CF-05 report comparison).
//!
//! A *diff* answers one question for CI: did this run get worse than the
//! baseline it is compared against? It is computed entirely offline from
//! two previously rendered JSON reports — nothing is re-run and no
//! network is touched.
//!
//! The model is deliberately small:
//!
//! - [`compare`] pairs the two reports' `results` by `(surface, testId)`
//!   and classifies every difference as a [`Transition`] (present in
//!   both, different status), a [`MissingEntry`] (present in the
//!   baseline only — coverage lost), or an [`AddedEntry`] (present in
//!   the current report only — new coverage, informational).
//! - The [`DiffReport`]'s [`DiffVerdict`] follows a strict precedence:
//!   regression > coverage-lost > improved > no-change.
//! - [`validate_eligibility`] refuses to compare two reports that are
//!   not comparable in the first place: different `schemaVersion`s,
//!   unsupported schema versions, or different `targetProtocol`s. That
//!   refusal is an [`IncompatibleReportsError`], which the CLI maps to
//!   CF-05's exit code 2.
//!
//! `stellar-canary diff` wires these together and renders the result as
//! `diffVersion: 1` JSON (see [`crate::json::DiffJsonReporter`]).

use canary_core::{CompatibilityResult, Status, Surface};

use crate::json::SCHEMA_VERSION;
use crate::ReportInput;

/// The current report-comparison schema version, rendered as the
/// `diffVersion` field of the comparison JSON.
pub const DIFF_VERSION: u32 = 1;

/// A stored report together with the `schemaVersion` it was written
/// under.
///
/// The schema version is retained (rather than checked at parse time)
/// precisely so eligibility can compare two reports written under
/// different schemas and explain the refusal, instead of one of them
/// failing to parse for an unrelated reason.
#[derive(Debug, Clone)]
pub struct StoredReport {
    pub schema_version: u32,
    pub input: ReportInput,
}

/// The overall outcome of a comparison, in strict precedence order:
/// `Regression` > `CoverageLost` > `Improved` > `NoChange`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffVerdict {
    /// Nothing differs between the two reports.
    NoChange,
    /// Every difference is an improvement; nothing regressed or vanished.
    Improved,
    /// At least one baseline test is absent from the current report
    /// (its coverage was lost), and nothing regressed.
    CoverageLost,
    /// At least one test's status got strictly worse.
    Regression,
}

impl DiffVerdict {
    /// The lowercase machine-readable form used in JSON output.
    pub fn as_str(&self) -> &'static str {
        match self {
            DiffVerdict::NoChange => "no-change",
            DiffVerdict::Improved => "improved",
            DiffVerdict::CoverageLost => "coverage-lost",
            DiffVerdict::Regression => "regression",
        }
    }

    /// Whether this verdict must fail CI gating — CF-05 exit code 2 for
    /// `stellar-canary diff`.
    pub fn is_failure(&self) -> bool {
        matches!(self, DiffVerdict::Regression | DiffVerdict::CoverageLost)
    }

    /// Parses the lowercase form produced by [`DiffVerdict::as_str`].
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "no-change" => Some(DiffVerdict::NoChange),
            "improved" => Some(DiffVerdict::Improved),
            "coverage-lost" => Some(DiffVerdict::CoverageLost),
            "regression" => Some(DiffVerdict::Regression),
            _ => None,
        }
    }
}

/// A test that ran in both reports with a different status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub surface: Surface,
    pub test_id: String,
    /// The status in the baseline report.
    pub from: Status,
    /// The status in the current report.
    pub to: Status,
}

impl Transition {
    /// A transition is a regression when the current status is strictly
    /// worse than the baseline status (see [`status_rank`]).
    pub fn is_regression(&self) -> bool {
        status_rank(self.to) < status_rank(self.from)
    }

    /// A transition is an improvement when the current status is
    /// strictly better than the baseline status.
    pub fn is_improvement(&self) -> bool {
        status_rank(self.to) > status_rank(self.from)
    }
}

/// A test present in the baseline report but absent from the current
/// one: the run's coverage shrank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingEntry {
    pub surface: Surface,
    pub test_id: String,
    /// The status the test had in the baseline report.
    pub baseline_status: Status,
}

/// A test present in the current report but absent from the baseline:
/// new coverage. Informational — it never affects the verdict, since
/// nothing that previously ran has gotten worse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedEntry {
    pub surface: Surface,
    pub test_id: String,
    pub status: Status,
}

/// The full comparison of two reports, as `diffVersion: 1` JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReport {
    pub diff_version: u32,
    pub verdict: DiffVerdict,
    /// Sorted by `(surface, testId)` — surfaces alphabetically
    /// (`rpc`, `soroban`, `xdr`), then test ids lexicographically.
    pub transitions: Vec<Transition>,
    /// Sorted by `(surface, testId)`.
    pub missing: Vec<MissingEntry>,
    /// Sorted by `(surface, testId)`.
    pub added: Vec<AddedEntry>,
}

/// How "healthy" a status is, for deciding whether a transition is a
/// regression or an improvement. Higher is better.
fn status_rank(status: Status) -> i8 {
    match status {
        Status::Pass => 4,
        Status::Warning => 3,
        Status::Fail => 2,
        Status::Error => 1,
        Status::Skipped => 0,
    }
}

/// Compares two eligible reports, entry by entry.
///
/// Entries are paired by `(surface, testId)` and the resulting vectors
/// are sorted by that same key (surfaces compare as their lowercase
/// names, so they come out alphabetically). Use [`compare_reports`] when
/// the reports have not been checked for eligibility yet.
pub fn compare(baseline: &ReportInput, current: &ReportInput) -> DiffReport {
    use std::collections::BTreeMap;

    let key = |result: &CompatibilityResult| (result.surface.to_string(), result.test_id.clone());
    let baseline_by_key: BTreeMap<(String, String), &CompatibilityResult> =
        baseline.results.iter().map(|r| (key(r), r)).collect();
    let current_by_key: BTreeMap<(String, String), &CompatibilityResult> =
        current.results.iter().map(|r| (key(r), r)).collect();

    let mut transitions = Vec::new();
    let mut missing = Vec::new();
    let mut added = Vec::new();

    // A BTreeMap iterates in key order, so both loops below produce
    // entries already sorted by (surface, testId) — the deterministic
    // order the JSON contract promises.
    for (map_key, baseline_result) in &baseline_by_key {
        match current_by_key.get(map_key) {
            Some(current_result) => {
                if current_result.status != baseline_result.status {
                    transitions.push(Transition {
                        surface: baseline_result.surface,
                        test_id: baseline_result.test_id.clone(),
                        from: baseline_result.status,
                        to: current_result.status,
                    });
                }
            }
            None => missing.push(MissingEntry {
                surface: baseline_result.surface,
                test_id: baseline_result.test_id.clone(),
                baseline_status: baseline_result.status,
            }),
        }
    }

    for (map_key, current_result) in &current_by_key {
        if !baseline_by_key.contains_key(map_key) {
            added.push(AddedEntry {
                surface: current_result.surface,
                test_id: current_result.test_id.clone(),
                status: current_result.status,
            });
        }
    }

    let verdict = verdict_for(&transitions, &missing);
    DiffReport {
        diff_version: DIFF_VERSION,
        verdict,
        transitions,
        missing,
        added,
    }
}

/// Applies the strict verdict precedence: regression beats coverage
/// loss, which beats improvement, which beats no change. Added entries
/// are informational and never influence the verdict.
fn verdict_for(transitions: &[Transition], missing: &[MissingEntry]) -> DiffVerdict {
    if transitions.iter().any(Transition::is_regression) {
        DiffVerdict::Regression
    } else if !missing.is_empty() {
        DiffVerdict::CoverageLost
    } else if transitions.iter().any(Transition::is_improvement) {
        DiffVerdict::Improved
    } else {
        DiffVerdict::NoChange
    }
}

/// Why two reports cannot be compared (CF-05 comparison eligibility).
///
/// The CLI maps this to CF-05's exit code 2 and prints the message on
/// stderr; no diff JSON is produced.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IncompatibleReportsError {
    #[error(
        "cannot compare reports: schemaVersion mismatch (baseline was written with \
         schemaVersion {baseline}, current with schemaVersion {current})"
    )]
    SchemaVersionMismatch { baseline: u32, current: u32 },

    #[error(
        "cannot compare reports: unsupported schemaVersion {found} (this build supports \
         schemaVersion {supported})"
    )]
    UnsupportedSchemaVersion { found: u32, supported: u32 },

    #[error(
        "cannot compare reports: protocol mismatch (baseline targets protocol {baseline}, \
         current targets protocol {current})"
    )]
    ProtocolMismatch { baseline: u32, current: u32 },
}

/// Enforces CF-05 comparison eligibility: both reports must have been
/// written under the same, currently supported `schemaVersion`, and must
/// target the same protocol.
///
/// Refusing here is the point — comparing Protocol 28 results against a
/// Protocol 29 baseline would otherwise produce a "diff" full of noise
/// that looks like a real regression.
pub fn validate_eligibility(
    baseline: &StoredReport,
    current: &StoredReport,
) -> Result<(), IncompatibleReportsError> {
    if baseline.schema_version != current.schema_version {
        return Err(IncompatibleReportsError::SchemaVersionMismatch {
            baseline: baseline.schema_version,
            current: current.schema_version,
        });
    }
    if baseline.schema_version != SCHEMA_VERSION {
        return Err(IncompatibleReportsError::UnsupportedSchemaVersion {
            found: baseline.schema_version,
            supported: SCHEMA_VERSION,
        });
    }
    if baseline.input.target_protocol != current.input.target_protocol {
        return Err(IncompatibleReportsError::ProtocolMismatch {
            baseline: baseline.input.target_protocol.0,
            current: current.input.target_protocol.0,
        });
    }
    Ok(())
}

/// Checks eligibility and, if the reports are comparable, compares them.
///
/// This is the one call `stellar-canary diff` makes; it exists so the
/// eligibility gate can never be skipped by accident.
pub fn compare_reports(
    baseline: StoredReport,
    current: StoredReport,
) -> Result<DiffReport, IncompatibleReportsError> {
    validate_eligibility(&baseline, &current)?;
    Ok(compare(&baseline.input, &current.input))
}

#[cfg(test)]
mod tests {
    use canary_core::{GitContext, PolicyDecision, ProjectType, ProtocolVersion};

    use super::*;

    fn result(surface: Surface, test_id: &str, status: Status) -> CompatibilityResult {
        CompatibilityResult {
            test_id: test_id.to_string(),
            protocol: ProtocolVersion(28),
            surface,
            status,
            summary: "mock".into(),
            details: None,
            duration_ms: 0,
            fixture_id: Some(test_id.to_string()),
        }
    }

    fn report(protocol: u32, results: Vec<CompatibilityResult>) -> ReportInput {
        ReportInput {
            tool_version: "0.0.0-test".into(),
            target_protocol: ProtocolVersion(protocol),
            project: crate::ProjectSummary {
                name: "mock".into(),
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

    #[test]
    fn identical_reports_produce_no_change_with_no_entries() {
        let results = vec![
            result(Surface::Xdr, "a", Status::Pass),
            result(Surface::Rpc, "b", Status::Fail),
        ];
        let diff = compare(&report(28, results.clone()), &report(28, results));
        assert_eq!(diff.verdict, DiffVerdict::NoChange);
        assert!(diff.transitions.is_empty());
        assert!(diff.missing.is_empty());
        assert!(diff.added.is_empty());
        assert_eq!(diff.diff_version, DIFF_VERSION);
    }

    #[test]
    fn a_worse_status_is_a_regression() {
        let baseline = report(28, vec![result(Surface::Xdr, "a", Status::Pass)]);
        let current = report(28, vec![result(Surface::Xdr, "a", Status::Fail)]);
        let diff = compare(&baseline, &current);
        assert_eq!(diff.verdict, DiffVerdict::Regression);
        assert_eq!(diff.transitions.len(), 1);
        assert!(diff.transitions[0].is_regression());
        assert!(!diff.transitions[0].is_improvement());
    }

    #[test]
    fn a_better_status_is_an_improvement() {
        let baseline = report(28, vec![result(Surface::Xdr, "a", Status::Fail)]);
        let current = report(28, vec![result(Surface::Xdr, "a", Status::Pass)]);
        let diff = compare(&baseline, &current);
        assert_eq!(diff.verdict, DiffVerdict::Improved);
        assert!(diff.transitions[0].is_improvement());
    }

    #[test]
    fn a_dropped_test_is_coverage_lost() {
        let baseline = report(
            28,
            vec![
                result(Surface::Xdr, "a", Status::Pass),
                result(Surface::Rpc, "b", Status::Pass),
            ],
        );
        let current = report(28, vec![result(Surface::Xdr, "a", Status::Pass)]);
        let diff = compare(&baseline, &current);
        assert_eq!(diff.verdict, DiffVerdict::CoverageLost);
        assert_eq!(diff.missing.len(), 1);
        assert_eq!(diff.missing[0].test_id, "b");
    }

    #[test]
    fn regression_outranks_coverage_loss_and_improvement() {
        let baseline = report(
            28,
            vec![
                result(Surface::Xdr, "keeps-getting-worse", Status::Pass),
                result(Surface::Rpc, "vanishes", Status::Pass),
            ],
        );
        let current = report(
            28,
            vec![
                result(Surface::Xdr, "keeps-getting-worse", Status::Fail),
                result(Surface::Soroban, "gets-better", Status::Fail),
            ],
        );
        // "gets-better" is an *added* entry, which never affects the
        // verdict; the transition is a regression and coverage was lost.
        let diff = compare(&baseline, &current);
        assert_eq!(diff.verdict, DiffVerdict::Regression);
    }

    #[test]
    fn coverage_loss_outranks_improvement() {
        let baseline = report(
            28,
            vec![
                result(Surface::Xdr, "improves", Status::Fail),
                result(Surface::Rpc, "vanishes", Status::Pass),
            ],
        );
        let current = report(28, vec![result(Surface::Xdr, "improves", Status::Pass)]);
        let diff = compare(&baseline, &current);
        assert_eq!(diff.verdict, DiffVerdict::CoverageLost);
    }

    #[test]
    fn entries_are_sorted_by_surface_alphabetically_then_test_id() {
        let baseline = report(28, Vec::new());
        let current = report(
            28,
            vec![
                result(Surface::Xdr, "t1", Status::Pass),
                result(Surface::Rpc, "t1", Status::Pass),
                result(Surface::Soroban, "t1", Status::Pass),
                result(Surface::Rpc, "a-first", Status::Pass),
            ],
        );
        let diff = compare(&baseline, &current);
        let order: Vec<(String, &str)> = diff
            .added
            .iter()
            .map(|a| (a.surface.to_string(), a.test_id.as_str()))
            .collect();
        assert_eq!(
            order,
            vec![
                ("rpc".to_string(), "a-first"),
                ("rpc".to_string(), "t1"),
                ("soroban".to_string(), "t1"),
                ("xdr".to_string(), "t1"),
            ]
        );
    }

    #[test]
    fn eligibility_accepts_matching_supported_reports() {
        let report = report(28, vec![result(Surface::Xdr, "a", Status::Pass)]);
        validate_eligibility(
            &stored(SCHEMA_VERSION, report.clone()),
            &stored(SCHEMA_VERSION, report),
        )
        .expect("matching reports are comparable");
    }

    #[test]
    fn eligibility_refuses_mismatched_schema_versions() {
        let report = report(28, Vec::new());
        let err = validate_eligibility(
            &stored(SCHEMA_VERSION, report.clone()),
            &stored(999, report),
        )
        .unwrap_err();
        assert_eq!(
            err,
            IncompatibleReportsError::SchemaVersionMismatch {
                baseline: SCHEMA_VERSION,
                current: 999,
            }
        );
        assert!(err.to_string().contains("schemaVersion mismatch"));
    }

    #[test]
    fn eligibility_refuses_an_unsupported_but_equal_schema_version() {
        let report = report(28, Vec::new());
        let err = validate_eligibility(&stored(999, report.clone()), &stored(999, report))
            .unwrap_err();
        assert!(matches!(
            err,
            IncompatibleReportsError::UnsupportedSchemaVersion {
                found: 999,
                supported: SCHEMA_VERSION
            }
        ));
    }

    #[test]
    fn eligibility_refuses_mismatched_protocols() {
        let baseline = report(28, vec![result(Surface::Xdr, "a", Status::Pass)]);
        let current = report(29, vec![result(Surface::Xdr, "a", Status::Pass)]);
        let err = validate_eligibility(
            &stored(SCHEMA_VERSION, baseline),
            &stored(SCHEMA_VERSION, current),
        )
        .unwrap_err();
        assert_eq!(
            err,
            IncompatibleReportsError::ProtocolMismatch {
                baseline: 28,
                current: 29,
            }
        );
        let message = err.to_string();
        assert!(message.contains("protocol mismatch"), "got: {message}");
        assert!(message.contains('28') && message.contains('29'), "got: {message}");
    }

    #[test]
    fn compare_reports_gates_on_eligibility() {
        let baseline = report(28, vec![result(Surface::Xdr, "a", Status::Pass)]);
        let current = report(29, vec![result(Surface::Xdr, "a", Status::Pass)]);
        assert!(compare_reports(
            stored(SCHEMA_VERSION, baseline),
            stored(SCHEMA_VERSION, current)
        )
        .is_err());

        let baseline = report(28, vec![result(Surface::Xdr, "a", Status::Pass)]);
        let current = report(28, vec![result(Surface::Xdr, "a", Status::Pass)]);
        let diff = compare_reports(
            stored(SCHEMA_VERSION, baseline),
            stored(SCHEMA_VERSION, current),
        )
        .expect("eligible reports compare");
        assert_eq!(diff.verdict, DiffVerdict::NoChange);
    }

    #[test]
    fn verdict_forms_round_trip_and_only_the_two_failures_gate_ci() {
        for verdict in [
            DiffVerdict::NoChange,
            DiffVerdict::Improved,
            DiffVerdict::CoverageLost,
            DiffVerdict::Regression,
        ] {
            assert_eq!(DiffVerdict::parse(verdict.as_str()), Some(verdict));
        }
        assert!(!DiffVerdict::NoChange.is_failure());
        assert!(!DiffVerdict::Improved.is_failure());
        assert!(DiffVerdict::CoverageLost.is_failure());
        assert!(DiffVerdict::Regression.is_failure());
        assert_eq!(DiffVerdict::parse("bogus"), None);
    }
}
