import assert from "node:assert/strict";
import test from "node:test";

import * as localHomeserverQa from "./local-homeserver-qa.mjs";

test("Synapse fixture pins matrixdotorg/synapse v1.157.0", () => {
  const dockerfile = localHomeserverQa.synapseDockerfile?.() ?? "";

  assert.match(dockerfile, /^FROM docker\.io\/matrixdotorg\/synapse:v1\.157\.0$/m);
});

test("Synapse positive fixture enables simplified Sliding Sync", () => {
  const config = localHomeserverQa.synapseEntrypoint?.() ?? "";

  assert.match(config, /experimental_features:\n(?: {2}.+\n)* {2}msc3575_enabled: true\n/);
});

test("Synapse negative fixture disables simplified Sliding Sync", () => {
  const config =
    localHomeserverQa.synapseEntrypoint?.({ slidingSyncEnabled: false }) ?? "";

  assert.match(config, /experimental_features:\n(?: {2}.+\n)* {2}msc3575_enabled: false\n/);
});

test("Tuwunel fixture intrinsically advertises simplified Sliding Sync", () => {
  const capabilities = localHomeserverQa.homeserverFixtureCapabilities?.("tuwunel");

  assert.deepEqual(capabilities, {
    simplifiedSlidingSync: {
      unstableFeature: "org.matrix.simplified_msc3575",
      enabled: true,
      configuration: "intrinsic"
    }
  });
});

test("server selection keeps individual Sliding Sync fixtures", () => {
  assert.deepEqual(localHomeserverQa.selectedServers?.("tuwunel"), ["tuwunel"]);
  assert.deepEqual(localHomeserverQa.selectedServers?.("synapse"), ["synapse"]);
});

test("both selects Tuwunel and Synapse without Conduit", () => {
  assert.deepEqual(localHomeserverQa.selectedServers?.("both"), ["tuwunel", "synapse"]);
});
