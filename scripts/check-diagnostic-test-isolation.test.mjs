#!/usr/bin/env node

import assert from "node:assert/strict";
import { test } from "node:test";

import { findDiagnosticTestIsolationViolations } from "./check-diagnostic-test-isolation.mjs";

test("detects a diagnostic snapshot test without the shared lock", () => {
  const source = `
    #[test]
    fn reads_diagnostics() {
      assert!(!koushi_diagnostics::snapshot().records.is_empty());
    }
  `;
  assert.deepEqual(findDiagnosticTestIsolationViolations(source, "fixture.rs"), [
    "fixture.rs:2:reads_diagnostics"
  ]);
});

test("accepts diagnostic snapshots under the shared lock", () => {
  const source = `
    #[tokio::test]
    async fn reads_diagnostics() {
      let _diagnostic_lock = koushi_diagnostics::test_support::lock();
      assert!(!koushi_diagnostics::snapshot().records.is_empty());
    }
  `;
  assert.deepEqual(findDiagnosticTestIsolationViolations(source, "fixture.rs"), []);
});

test("does not confuse a nested ignored child with its parent test", () => {
  const source = `
    #[test]
    fn launches_child() {
      std::process::Command::new("test").status().unwrap();
    }

    #[test]
    #[ignore]
    fn child() {
      println!("{}", koushi_diagnostics::snapshot().records.len());
    }
  `;
  assert.deepEqual(findDiagnosticTestIsolationViolations(source, "fixture.rs"), [
    "fixture.rs:7:child"
  ]);
});
