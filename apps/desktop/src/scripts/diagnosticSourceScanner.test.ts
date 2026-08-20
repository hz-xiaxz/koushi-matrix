import { describe,expect,test } from "vitest";
import {
GATED_DIAGNOSTIC_REASON,
SYNTHETIC_TRACE_DECLARATION,
runtimeDiagnosticStderrFindings,
runtimeRustSources,
scanDiagnosticSources
} from "./diagnosticSourceScanner";

describe("diagnostic source scanner", () => {
  test(
    "always-on diagnostic collection rejects trace-only producers and accepts stderr mirrors",
    { timeout: 60_000 },
    () => {
    const badFixture = `
fn gated_only() {
  if std::env::var_os("KOUSHI_SYNTH_TRACE").is_some() {
    record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", "gated"));
    eprintln!("synthetic stderr mirror");
  }
}
`;
    const goodFixture = `
fn collected_first() {
  let stage = "collected";
  record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage));
  if std::env::var_os("KOUSHI_SYNTH_TRACE").is_some() {
    eprintln!("synthetic stderr stage={stage}");
  }
}

#[cfg(test)]
mod tests {
  fn test_only_environment_probe() {
    if std::env::var_os("KOUSHI_SYNTH_TRACE").is_some() {
      eprintln!("test-only");
    }
  }
}
`;
    const helperAndAliasFixture = `
const SYNTHETIC_TRACE_ENV: &str = "KOUSHI_SYNTH_TRACE";

fn stderr_enabled() -> bool {
  std::env::var_os(SYNTHETIC_TRACE_ENV).is_some()
}

fn helper_gated_only() {
  if stderr_enabled() {
    record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", "helper"));
    eprintln!("synthetic helper stderr mirror");
  }
}

fn boolean_alias_gated_only() {
  let trace = std::env::var_os("KOUSHI_SYNTH_TRACE").is_some();
  if trace {
    record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", "alias"));
    eprintln!("synthetic alias stderr mirror");
  }
}

fn collected_helper_mirror(stage: &'static str) {
  record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage));
  if stderr_enabled() {
    eprintln!("synthetic helper stderr stage={stage}");
  }
}

fn collected_alias_mirror() {
  let trace = std::env::var_os("KOUSHI_SYNTH_TRACE").is_some();
  let stage = "alias_collected";
  record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage));
  if trace {
    eprintln!("synthetic alias stderr stage={stage}");
  }
}
`;

    const badFindings = scanDiagnosticSources([
      { relativePath: "fixtures/bad.rs", source: badFixture }
    ]);
    expect(badFindings).toHaveLength(1);
    expect(badFindings[0]).toMatchObject({
      relativePath: "fixtures/bad.rs",
      line: 3,
      location: "fixtures/bad.rs:3"
    });
    expect(badFindings[0].reason).toContain("structured collection");

    const helperAndAliasFindings = scanDiagnosticSources([
      {
        relativePath: "fixtures/helper-and-alias.rs",
        source: helperAndAliasFixture
      }
    ]);
    expect(helperAndAliasFindings).toHaveLength(2);
    expect(
      helperAndAliasFindings.every((finding) => finding.relativePath.includes("fixtures/"))
    ).toBe(true);
    expect(helperAndAliasFindings.every((finding) => finding.line > 0)).toBe(true);
    expect(helperAndAliasFindings.every((finding) => finding.location.includes(":"))).toBe(true);
    expect(
      helperAndAliasFindings.every((finding) => finding.reason === GATED_DIAGNOSTIC_REASON)
    ).toBe(true);

    expect(
      scanDiagnosticSources([{ relativePath: "fixtures/good.rs", source: goodFixture }])
    ).toEqual([]);

    const runtimeFindings = scanDiagnosticSources(runtimeRustSources());
    expect(runtimeFindings).toEqual([]);
  });

  test("scanner rejects structured producers inside every recognized gate form without stderr", () => {
    const directGateFixture = `
fn direct_gate_only() {
  if std::env::var_os("KOUSHI_SYNTH_TRACE").is_some() {
    record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", "direct"));
  }
}
`;
    const helperGateFixture = `
fn record_helper() {
  record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", "helper"));
}

fn helper_gate_only() {
  if stderr_enabled() {
    record_helper();
  }
}

fn stderr_enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}
`;
    const booleanAliasGateFixture = `
fn boolean_alias_gate_only() {
  let trace = std::env::var_os("KOUSHI_SYNTH_TRACE").is_some();
  if trace {
    record_helper();
  }
}

fn record_helper() {
  record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", "alias"));
}
`;

    for (const [relativePath, source, line] of [
      ["fixtures/direct-gate-only.rs", directGateFixture, 3],
      ["fixtures/helper-gate-only.rs", helperGateFixture, 7],
      ["fixtures/boolean-alias-gate-only.rs", booleanAliasGateFixture, 4]
    ] as const) {
      const findings = scanDiagnosticSources([{ relativePath, source }]);
      expect(findings).toHaveLength(1);
      expect(findings[0]).toMatchObject({
        relativePath,
        line,
        location: `${relativePath}:${line}`
      });
      expect(findings[0].reason).toContain("structured collection");
    }
  });

  test("scanner does not let an unrelated record hide a later gated-only diagnostic", () => {
    const fixture = `
${SYNTHETIC_TRACE_DECLARATION}

fn trace_enabled() -> bool {
  std::env::var_os(SYNTHETIC_TRACE_ENV).is_some()
}

fn unrelated_canonical_record_before_gate() {
  record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", "unrelated"));
  if trace_enabled() {
    eprintln!("synthetic stderr mirror");
  }
}

fn unrelated_canonical_record_helper() {
  record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", "helper_unrelated"));
}

fn unrelated_canonical_helper_before_gate() {
  unrelated_canonical_record_helper();
  if trace_enabled() {
    eprintln!("synthetic stderr mirror");
  }
}
`;

    const findings = scanDiagnosticSources([
      { relativePath: "fixtures/unrelated-record.rs", source: fixture }
    ]);
    expect(findings).toHaveLength(2);
    expect(findings.map((finding) => finding.line)).toEqual([10, 21]);
    expect(findings).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          relativePath: "fixtures/unrelated-record.rs",
          location: "fixtures/unrelated-record.rs:10"
        }),
        expect.objectContaining({
          relativePath: "fixtures/unrelated-record.rs",
          location: "fixtures/unrelated-record.rs:21"
        })
      ])
    );
  });

  test("scanner association uses producer arguments and stops at control-flow barriers", () => {
    const reboundStageFixture = `
${SYNTHETIC_TRACE_DECLARATION}

fn trace_enabled() -> bool {
  std::env::var_os(SYNTHETIC_TRACE_ENV).is_some()
}

fn unrelated_then_rebound_stage() {
  record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", "unrelated"));
  let stage = "actual_mirror";
  if trace_enabled() {
    eprintln!("synthetic stderr stage={stage}");
  }
}
`;
    const gateNameCollisionFixture = `
${SYNTHETIC_TRACE_DECLARATION}

fn trace_enabled() -> bool {
  std::env::var_os(SYNTHETIC_TRACE_ENV).is_some()
}

fn unrelated_trace_stage() {
  record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", "trace"));
  if trace_enabled() {
    eprintln!("synthetic stderr mirror");
  }
}
`;
    const conditionalCollectorFixture = `
${SYNTHETIC_TRACE_DECLARATION}

fn trace_enabled() -> bool {
  std::env::var_os(SYNTHETIC_TRACE_ENV).is_some()
}

fn conditionally_collected(collect: bool) {
  let stage = "conditional";
  if collect {
    record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage));
  }
  if trace_enabled() {
    eprintln!("synthetic stderr stage={stage}");
  }
}
`;

    const findings = scanDiagnosticSources([
      {
        relativePath: "fixtures/rebound-stage.rs",
        source: reboundStageFixture
      },
      {
        relativePath: "fixtures/gate-name-collision.rs",
        source: gateNameCollisionFixture
      },
      {
        relativePath: "fixtures/conditional-collector.rs",
        source: conditionalCollectorFixture
      }
    ]);
    expect(findings.map((finding) => finding.relativePath)).toEqual([
      "fixtures/rebound-stage.rs",
      "fixtures/gate-name-collision.rs",
      "fixtures/conditional-collector.rs"
    ]);
    expect(findings.every((finding) => finding.reason === GATED_DIAGNOSTIC_REASON)).toBe(true);
  });

  test("scanner rejects opposite-polarity conditional collection", () => {
    const fixture = `
fn trace_enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}

fn negated_condition(collect: bool) {
  let stage = "negated";
  if collect {
    record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage));
  }
  if !collect && trace_enabled() {
    eprintln!("synthetic stderr stage={stage}");
  }
}
`;

    const findings = scanDiagnosticSources([
      { relativePath: "fixtures/negated-condition.rs", source: fixture }
    ]);
    expect(findings).toHaveLength(1);
    expect(findings[0]).toMatchObject({
      relativePath: "fixtures/negated-condition.rs",
      reason: GATED_DIAGNOSTIC_REASON
    });
  });

  test("scanner rejects disjunctive gates and accepts implied conjunctions", () => {
    const fixture = `
fn trace_enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}

fn disjunctive_gate(collect: bool) {
  let stage = "disjunction";
  if collect {
    record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage));
  }
  if trace_enabled() || collect {
    eprintln!("synthetic stderr stage={stage}");
  }
}

fn paired_condition(collect: bool, ready: bool) {
  let stage = "paired_condition";
  if ready && collect {
    record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage));
  }
  if trace_enabled() && collect && ready {
    eprintln!("synthetic stderr stage={stage}");
  }
}
`;

    const findings = scanDiagnosticSources([
      { relativePath: "fixtures/disjunctive-and-implied.rs", source: fixture }
    ]);
    expect(findings).toHaveLength(1);
    expect(findings[0]).toMatchObject({
      relativePath: "fixtures/disjunctive-and-implied.rs",
      reason: GATED_DIAGNOSTIC_REASON
    });
  });

  test("scanner treats collector loops as barriers unless iterators are paired", () => {
    const fixture = `
fn trace_enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}

fn independent_loop(items: &[&str]) {
  let stage = "independent_loop";
  for _item in items {
    record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage));
  }
  if trace_enabled() {
    eprintln!("synthetic stderr stage={stage}");
  }
}

fn paired_loop(items: &[&str]) {
  let stage = "paired_loop";
  for item in items {
    record(make_diagnostic_event(stage, item));
  }
  if trace_enabled() {
    for item in items {
      eprintln!("synthetic stderr stage={stage} item={item}");
    }
  }
}
`;

    const findings = scanDiagnosticSources([
      { relativePath: "fixtures/paired-and-independent-loops.rs", source: fixture }
    ]);
    expect(findings).toHaveLength(1);
    expect(findings[0]).toMatchObject({
      relativePath: "fixtures/paired-and-independent-loops.rs",
      reason: GATED_DIAGNOSTIC_REASON
    });
  });

  test("scanner pairs collector loops only through equivalent iterator data flow", () => {
    const fixture = `
fn trace_enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}

fn different_iterators(collected_items: &[&str], mirrored_items: &[&str]) {
  let stage = "different_iterators";
  for item in collected_items.iter() {
    record(make_diagnostic_event(stage, item));
  }
  if trace_enabled() {
    for item in mirrored_items.iter() {
      eprintln!("synthetic stderr stage={stage} item={item}");
    }
  }
}

fn same_iterator(items: &[&str]) {
  let stage = "same_iterator";
  for item in items.iter() {
    record(make_diagnostic_event(stage, item));
  }
  if trace_enabled() {
    for item in items.iter() {
      eprintln!("synthetic stderr stage={stage} item={item}");
    }
  }
}

fn aliased_iterator(collected_items: &[&str]) {
  let stage = "aliased_iterator";
  let mirrored_items = collected_items;
  for item in collected_items.iter() {
    record(make_diagnostic_event(stage, item));
  }
  if trace_enabled() {
    for item in mirrored_items.iter() {
      eprintln!("synthetic stderr stage={stage} item={item}");
    }
  }
}
`;

    const findings = scanDiagnosticSources([
      { relativePath: "fixtures/equivalent-loop-iterators.rs", source: fixture }
    ]);
    expect(findings).toHaveLength(1);
    expect(findings[0]).toMatchObject({
      relativePath: "fixtures/equivalent-loop-iterators.rs",
      reason: GATED_DIAGNOSTIC_REASON
    });
  });

  test("scanner recognizes generic gated record producers without stderr", () => {
    const fixture = `
${SYNTHETIC_TRACE_DECLARATION}

fn trace_enabled() -> bool {
  std::env::var_os(SYNTHETIC_TRACE_ENV).is_some()
}

fn make_diagnostic_event() -> DiagnosticEvent {
  todo!()
}

fn generic_direct_record_only() {
  if trace_enabled() {
    record(make_diagnostic_event());
  }
}

fn generic_record_helper() {
  record(make_diagnostic_event());
}

fn generic_helper_record_only() {
  if trace_enabled() {
    generic_record_helper();
  }
}
`;

    const findings = scanDiagnosticSources([
      { relativePath: "fixtures/generic-record-only.rs", source: fixture }
    ]);
    expect(findings).toHaveLength(2);
    expect(findings.map((finding) => finding.line)).toEqual([13, 23]);
    expect(findings.every((finding) => finding.reason === GATED_DIAGNOSTIC_REASON)).toBe(true);
  });

  test("scanner accepts semantically linked generic records, wrappers, and event bindings", () => {
    const fixture = `
${SYNTHETIC_TRACE_DECLARATION}

fn trace_enabled() -> bool {
  std::env::var_os(SYNTHETIC_TRACE_ENV).is_some()
}

fn make_diagnostic_event(stage: &'static str) -> DiagnosticEvent {
  todo!()
}

fn record_generic(stage: &'static str) {
  record(make_diagnostic_event(stage));
}

fn record_wrapper(stage: &'static str) {
  record_generic(stage);
}

fn direct_generic_mirror() {
  let stage = "direct_generic";
  record(make_diagnostic_event(stage));
  if trace_enabled() {
    eprintln!("synthetic stderr stage={stage}");
  }
}

fn wrapped_generic_mirror() {
  let stage = "wrapped_generic";
  record_wrapper(stage);
  if trace_enabled() {
    eprintln!("synthetic stderr stage={stage}");
  }
}

fn arbitrary_event_binding_mirror() {
  let stage = "bound_generic";
  let diagnostic_entry = make_diagnostic_event(stage);
  record(diagnostic_entry);
  if trace_enabled() {
    eprintln!("synthetic stderr stage={stage}");
  }
}
`;

    expect(
      scanDiagnosticSources([{ relativePath: "fixtures/generic-mirrors.rs", source: fixture }])
    ).toEqual([]);
  });

  test("scanner recognizes record batches and event-vector data flow", () => {
    const goodFixture = `
fn trace_enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}

fn collected_batch_before_early_exit() {
  let stage = "batch";
  let mut diagnostic_events = vec![
    DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage),
  ];
  diagnostic_events.push(make_diagnostic_event(stage));
  record_batch(diagnostic_events);
  if !trace_enabled() {
    return;
  }
  eprintln!("synthetic stderr stage={stage}");
}
`;
    const badFixture = `
fn trace_enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}

fn gated_batch_only() {
  let stage = "gated_batch";
  if trace_enabled() {
    let diagnostic_events = vec![make_diagnostic_event(stage)];
    record_batch(diagnostic_events);
  }
}
`;

    expect(
      scanDiagnosticSources([
        { relativePath: "fixtures/good-record-batch.rs", source: goodFixture }
      ])
    ).toEqual([]);
    const badFindings = scanDiagnosticSources([
      { relativePath: "fixtures/bad-record-batch.rs", source: badFixture }
    ]);
    expect(badFindings).toHaveLength(1);
    expect(badFindings[0]).toMatchObject({
      relativePath: "fixtures/bad-record-batch.rs",
      reason: GATED_DIAGNOSTIC_REASON
    });
  });

  test("stderr helper discovery follows two-hop chains without masking gated-only output", () => {
    const fixture = `
fn stderr_leaf(stage: &'static str) {
  eprintln!("synthetic stderr stage={stage}");
}

fn stderr_middle(stage: &'static str) {
  stderr_leaf(stage);
}

fn trace_enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}

fn gated_two_hop_only() {
  if trace_enabled() {
    stderr_middle("gated");
  }
}

fn collected_two_hop_mirror() {
  let stage = "collected";
  record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage));
  if trace_enabled() {
    stderr_middle(stage);
  }
}
`;

    const findings = scanDiagnosticSources([
      { relativePath: "fixtures/two-hop-stderr.rs", source: fixture }
    ]);
    expect(findings).toHaveLength(1);
    expect(findings[0]).toMatchObject({
      relativePath: "fixtures/two-hop-stderr.rs",
      line: 15,
      location: "fixtures/two-hop-stderr.rs:15"
    });
  });

  test("scanner recognizes balanced multiline gates and negative early-exit gates", () => {
    const multilineBadFixture = `
fn multiline_gate_only() {
  let stage = "multiline";
  if std::env::var_os(
    "KOUSHI_SYNTH_TRACE"
  ).is_some() {
    eprintln!("synthetic stderr stage={stage}");
  }
}
`;
    const negativeEprintlnBadFixture = `
fn trace_enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}

fn early_exit_gate_only() {
  let stage = "early_exit";
  if !trace_enabled() {
    return;
  }
  eprintln!("synthetic stderr stage={stage}");
}
`;
    const negativeHelperBadFixture = `
fn trace_enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}

fn stderr_leaf(stage: &'static str) {
  eprintln!("synthetic stderr stage={stage}");
}

fn early_exit_helper_only() {
  let stage = "early_exit_helper";
  if !trace_enabled() {
    return;
  }
  stderr_leaf(stage);
}
`;
    const goodFixture = `
fn trace_enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}

fn stderr_leaf(stage: &'static str) {
  eprintln!("synthetic stderr stage={stage}");
}

fn collected_before_multiline_gate() {
  let stage = "multiline_collected";
  record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage));
  if std::env::var_os(
    "KOUSHI_SYNTH_TRACE"
  ).is_some() {
    eprintln!("synthetic stderr stage={stage}");
  }
}

fn collected_before_early_exit() {
  let stage = "early_exit_collected";
  record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage));
  if !trace_enabled() {
    return;
  }
  stderr_leaf(stage);
}
`;

    const badFindings = scanDiagnosticSources([
      {
        relativePath: "fixtures/multiline-gate-only.rs",
        source: multilineBadFixture
      },
      {
        relativePath: "fixtures/negative-eprintln-gate-only.rs",
        source: negativeEprintlnBadFixture
      },
      {
        relativePath: "fixtures/negative-helper-gate-only.rs",
        source: negativeHelperBadFixture
      }
    ]);
    expect(badFindings.map((finding) => finding.relativePath)).toEqual([
      "fixtures/multiline-gate-only.rs",
      "fixtures/negative-eprintln-gate-only.rs",
      "fixtures/negative-helper-gate-only.rs"
    ]);
    expect(badFindings.every((finding) => finding.reason === GATED_DIAGNOSTIC_REASON)).toBe(true);
    expect(
      scanDiagnosticSources([
        {
          relativePath: "fixtures/multiline-and-early-good.rs",
          source: goodFixture
        }
      ])
    ).toEqual([]);
  });

  test("scanner resolves module-qualified environment and diagnostic helpers across files", () => {
    const unreadTraceFixture = `
const ENV_VAR: &str = "KOUSHI_UNREAD_TRACE";

pub(crate) fn enabled() -> bool {
  std::env::var_os(ENV_VAR).is_some()
}

pub(crate) fn trace_room_list_applied(stage: &'static str) {
  record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage));
}
`;
    const runtimeFixture = `
fn reduce_app_action(action: Action) {
  let stage = "room_list_applied";
  let raw_room_list_trace = if unread_trace::enabled()
    && matches!(action, Action::RoomListUpdated)
  {
    Some(stage)
  } else {
    None
  };
  if let Some(raw_stage) = raw_room_list_trace {
    unread_trace::trace_room_list_applied(raw_stage);
  }
}
`;

    const findings = scanDiagnosticSources([
      { relativePath: "fixtures/unread_trace.rs", source: unreadTraceFixture },
      { relativePath: "fixtures/runtime.rs", source: runtimeFixture }
    ]);
    expect(findings).toHaveLength(1);
    expect(findings[0]).toMatchObject({
      relativePath: "fixtures/runtime.rs",
      location: "fixtures/runtime.rs:11",
      reason: GATED_DIAGNOSTIC_REASON
    });
  });

  test("scanner follows wrapped environment helpers and two-hop Self record wrappers", () => {
    const fixture = `
fn direct_env_enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}

fn trace_enabled() -> bool {
  direct_env_enabled()
}

fn record_leaf(stage: &'static str) {
  record(make_diagnostic_event(stage));
}

fn record_wrapper(stage: &'static str) {
  Self::record_leaf(stage);
}

fn wrapped_gate_only() {
  let stage = "wrapped_gate";
  if trace_enabled() {
    Self::record_wrapper(stage);
  }
}
`;

    const findings = scanDiagnosticSources([
      { relativePath: "fixtures/wrapped-helper-chain.rs", source: fixture }
    ]);
    expect(findings).toHaveLength(1);
    expect(findings[0]).toMatchObject({
      relativePath: "fixtures/wrapped-helper-chain.rs",
      reason: GATED_DIAGNOSTIC_REASON
    });
  });

  test("scanner follows arbitrary and transitive environment aliases", () => {
    const fixture = `
fn arbitrary_alias_gate_only() {
  let stage = "arbitrary_alias";
  let gate = std::env::var_os("KOUSHI_SYNTH_TRACE").is_some();
  let forwarded = gate;
  if forwarded {
    record(make_diagnostic_event(stage));
  }
}
`;

    const findings = scanDiagnosticSources([
      { relativePath: "fixtures/arbitrary-alias-chain.rs", source: fixture }
    ]);
    expect(findings).toHaveLength(1);
    expect(findings[0]).toMatchObject({
      relativePath: "fixtures/arbitrary-alias-chain.rs",
      reason: GATED_DIAGNOSTIC_REASON
    });
  });

  test("scanner normalizes crate-qualified cross-file environment helpers", () => {
    const crossFileHelperFixture = `
pub(crate) fn enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}
`;
    const crossFileRuntimeFixture = `
fn crate_qualified_gate_only() {
  let stage = "crate_qualified";
  if crate::trace_gate::enabled() {
    record(make_diagnostic_event(stage));
  }
}
`;

    const findings = scanDiagnosticSources([
      { relativePath: "fixtures/trace_gate.rs", source: crossFileHelperFixture },
      { relativePath: "fixtures/qualified-runtime.rs", source: crossFileRuntimeFixture }
    ]);
    expect(findings).toHaveLength(1);
    expect(findings[0]).toMatchObject({
      relativePath: "fixtures/qualified-runtime.rs",
      reason: GATED_DIAGNOSTIC_REASON
    });
  });

  test("scanner preserves nested module identity for cross-file helpers", () => {
    const nestedHelperFixture = `
pub(crate) fn enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}
`;
    const crateNestedRuntimeFixture = `
fn crate_nested_gate_only() {
  if crate::diagnostics::trace_gate::enabled() {
    record(make_diagnostic_event("crate_nested"));
  }
}
`;
    const selfNestedRuntimeFixture = `
fn self_nested_gate_only() {
  if self::diagnostics::trace_gate::enabled() {
    record(make_diagnostic_event("self_nested"));
  }
}
`;
    const superNestedRuntimeFixture = `
fn super_nested_gate_only() {
  if super::trace_gate::enabled() {
    record(make_diagnostic_event("super_nested"));
  }
}
`;

    const findings = scanDiagnosticSources([
      {
        relativePath: "fixtures/diagnostics/trace_gate.rs",
        source: nestedHelperFixture
      },
      { relativePath: "fixtures/crate-runtime.rs", source: crateNestedRuntimeFixture },
      { relativePath: "fixtures/lib.rs", source: selfNestedRuntimeFixture },
      {
        relativePath: "fixtures/diagnostics/super-runtime.rs",
        source: superNestedRuntimeFixture
      }
    ]);
    expect(findings).toHaveLength(3);
    expect(findings.every((finding) => finding.reason === GATED_DIAGNOSTIC_REASON)).toBe(true);

    const wrongModuleFindings = scanDiagnosticSources([
      { relativePath: "fixtures/other/trace_gate.rs", source: nestedHelperFixture },
      {
        relativePath: "fixtures/wrong-module-runtime.rs",
        source: `
fn wrong_module_gate_only() {
  if crate::diagnostics::trace_gate::enabled() {
    record(make_diagnostic_event("wrong_module"));
  }
}
`
      }
    ]);
    expect(wrongModuleFindings).toHaveLength(0);
  });

  test("scanner keeps balanced one-line scopes from duplicating findings", () => {
    const fixture = `
fn trace_enabled() -> bool { std::env::var_os("KOUSHI_SYNTH_TRACE").is_some() }
fn one_line_gate_only() { if trace_enabled() { record(make_diagnostic_event("one_line")); } }
`;

    const findings = scanDiagnosticSources([
      { relativePath: "fixtures/one-line-scopes.rs", source: fixture }
    ]);
    expect(findings).toHaveLength(1);
    expect(findings[0]).toMatchObject({
      relativePath: "fixtures/one-line-scopes.rs",
      line: 3,
      location: "fixtures/one-line-scopes.rs:3",
      reason: GATED_DIAGNOSTIC_REASON
    });
  });

  test("scanner ignores record and helper spellings inside comments and strings", () => {
    const lineCommentFixture = `
fn trace_enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}

fn comment_is_not_collection() {
  let stage = "comment";
  // record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage));
  if trace_enabled() {
    eprintln!("synthetic stderr stage={stage}");
  }
}
`;
    const blockCommentFixture = `
fn trace_enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}

fn block_comment_is_not_collection() {
  let stage = "block_comment";
  /* record(DiagnosticEvent::new(
    DiagnosticLevel::Debug,
    "synthetic",
    stage,
  )); */
  if trace_enabled() {
    eprintln!("synthetic stderr stage={stage}");
  }
}
`;
    const stringHelperFixture = `
fn trace_enabled() -> bool {
  std::env::var_os("KOUSHI_SYNTH_TRACE").is_some()
}

fn fake_record_helper(stage: &'static str) {
  let example = "record(DiagnosticEvent::new(DiagnosticLevel::Debug, synthetic, stage))";
}

fn string_is_not_helper_collection() {
  let stage = "string_helper";
  fake_record_helper(stage);
  if trace_enabled() {
    eprintln!("synthetic stderr stage={stage}");
  }
}
`;

    const findings = scanDiagnosticSources([
      {
        relativePath: "fixtures/line-comment-record.rs",
        source: lineCommentFixture
      },
      {
        relativePath: "fixtures/block-comment-record.rs",
        source: blockCommentFixture
      },
      {
        relativePath: "fixtures/string-helper-record.rs",
        source: stringHelperFixture
      }
    ]);
    expect(findings.map((finding) => finding.relativePath)).toEqual([
      "fixtures/line-comment-record.rs",
      "fixtures/block-comment-record.rs",
      "fixtures/string-helper-record.rs"
    ]);
    expect(findings.every((finding) => finding.reason === GATED_DIAGNOSTIC_REASON)).toBe(true);
  });

  test("scanner accepts direct, helper, loop, and transformed mirror siblings", () => {
    const fixture = `
${SYNTHETIC_TRACE_DECLARATION}

fn trace_enabled() -> bool {
  std::env::var_os(SYNTHETIC_TRACE_ENV).is_some()
}

fn record_helper(stage: &'static str) {
  record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage));
}

fn direct_mirror() {
  let stage = "direct";
  record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage));
  if trace_enabled() {
    eprintln!("synthetic stderr stage={stage}");
  }
}

fn helper_mirror() {
  let stage = "helper";
  record_helper(stage);
  if trace_enabled() {
    eprintln!("synthetic stderr stage={stage}");
  }
}

fn loop_mirror(items: &[&'static str]) {
  let stage = "loop";
  for item in items {
    record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", stage));
  }
  if trace_enabled() {
    for item in items {
      eprintln!("synthetic stderr stage={stage} item={item}");
    }
  }
}

fn transformed_mirror() {
  let stage = "transformed";
  let event = make_diagnostic_event(stage);
  record(event);
  let line = format!("stage={stage}");
  if trace_enabled() {
    eprintln!("{line}");
  }
}
`;

    expect(
      scanDiagnosticSources([{ relativePath: "fixtures/mirror-shapes.rs", source: fixture }])
    ).toEqual([]);
  });

  test("scanner masks only cfg items that are provably test-only", () => {
    const fixture = `
#[cfg(test)]
fn exact_test_only() {
  if std::env::var_os("KOUSHI_SYNTH_TRACE").is_some() {
    record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", "exact-test"));
    eprintln!("test-only");
  }
}

#[cfg(all(test, feature = "diagnostic-runtime"))]
fn all_test_only() {
  if std::env::var_os("KOUSHI_SYNTH_TRACE").is_some() {
    record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", "all-test"));
    eprintln!("test-only");
  }
}

#[cfg(any(test, feature = "diagnostic-runtime"))]
fn conditional_runtime() {
  if std::env::var_os("KOUSHI_SYNTH_TRACE").is_some() {
    record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", "conditional"));
    eprintln!("synthetic stderr mirror");
  }
}

#[cfg(all(any(test, feature = "diagnostic-runtime"), test))]
fn nested_all_test_only() {
  if std::env::var_os("KOUSHI_SYNTH_TRACE").is_some() {
    record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", "nested-test"));
    eprintln!("test-only");
  }
}
`;

    const findings = scanDiagnosticSources([
      { relativePath: "fixtures/cfg-conditions.rs", source: fixture }
    ]);
    expect(findings).toHaveLength(1);
    expect(findings[0]).toMatchObject({
      relativePath: "fixtures/cfg-conditions.rs",
      line: 20,
      location: "fixtures/cfg-conditions.rs:20"
    });
  });

  test("scanner keeps production code after a balanced test-only module", () => {
    const fixture = `
#[cfg(test)]
mod tests {
  fn test_only_environment_probe() {
    if std::env::var_os("KOUSHI_SYNTH_TRACE").is_some() {
      eprintln!("test-only");
    }
  }
}

fn production_after_tests() {
  if std::env::var_os("KOUSHI_SYNTH_TRACE").is_some() {
    record(DiagnosticEvent::new(DiagnosticLevel::Debug, "synthetic", "after-tests"));
    eprintln!("synthetic stderr mirror");
  }
}
`;

    const findings = scanDiagnosticSources([
      { relativePath: "fixtures/production-after-tests.rs", source: fixture }
    ]);
    expect(findings).toHaveLength(1);
    expect(findings[0]).toMatchObject({
      relativePath: "fixtures/production-after-tests.rs",
      line: 12,
      location: "fixtures/production-after-tests.rs:12"
    });
  });

  test("application runtime has no diagnostic stderr mirror or trace environment gate", () => {
    expect(runtimeDiagnosticStderrFindings(runtimeRustSources())).toEqual([]);
  });

  test("runtime stderr scanner ignores comments and strings but catches real syntax", () => {
    const fixture = `// eprintln!("comment")
const PRINTED = "eprintln!(not a macro)";
const LITERAL = "KOUSHI_SYNC_TRACE";
const TRACE: &str = "KOUSHI_SYNC_TRACE";
fn warning() { eprintln!("real"); }
fn gate() {
  if std::env::var_os("KOUSHI_SEARCH_TRACE").is_some() {}
}
const MULTILINE_TRACE: &str =
  "KOUSHI_UNREAD_TRACE";
fn multiline_gate() {
  if std::env::var_os(
    "KOUSHI_STARTUP_TRACE",
  ).is_some() {}
}
fn block_comment_gate() {
  if std::env::var_os(
    /*
     "KOUSHI_STARTUP_TRACE"
     */
    "SOME_OTHER_ENV",
  ).is_some() {}
}
#[cfg(test)]
fn test_only() {
  eprintln!("test only");
  let literal = "KOUSHI_UNREAD_TRACE";
}`;

    expect(
      runtimeDiagnosticStderrFindings([{ relativePath: "fixtures/runtime-stderr.rs", source: fixture }])
    ).toEqual([
      {
        relativePath: "fixtures/runtime-stderr.rs",
        line: 4,
        location: "fixtures/runtime-stderr.rs:4",
        reason: "runtime diagnostic environment gate remains"
      },
      {
        relativePath: "fixtures/runtime-stderr.rs",
        line: 5,
        location: "fixtures/runtime-stderr.rs:5",
        reason: "runtime diagnostic writes to stderr"
      },
      {
        relativePath: "fixtures/runtime-stderr.rs",
        line: 7,
        location: "fixtures/runtime-stderr.rs:7",
        reason: "runtime diagnostic environment gate remains"
      },
      {
        relativePath: "fixtures/runtime-stderr.rs",
        line: 10,
        location: "fixtures/runtime-stderr.rs:10",
        reason: "runtime diagnostic environment gate remains"
      },
      {
        relativePath: "fixtures/runtime-stderr.rs",
        line: 13,
        location: "fixtures/runtime-stderr.rs:13",
        reason: "runtime diagnostic environment gate remains"
      }
    ]);
  });

});
