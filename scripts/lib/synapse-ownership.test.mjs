import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { once } from "node:events";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import * as fixture from "./local-homeserver-qa.mjs";

test("Synapse runtime identity avoids upstream chown for non-root POSIX users", () => {
  assert.deepEqual(fixture.synapseRuntimeIdentity({ uid: 12345, gid: 23456 }), [
    "--user", "12345:23456"
  ]);
  assert.deepEqual(fixture.synapseRuntimeIdentity({ uid: 0, gid: 23456 }), [
    "--user", "0:23456", "-e", "UID=0", "-e", "GID=23456"
  ]);
  assert.deepEqual(fixture.synapseRuntimeIdentity({ uid: null, gid: null }), [
    "--user", "991:991"
  ]);
});

test("Synapse launcher passes invoking identity to the actual Docker runner", async () => {
  const directory = mkdtempSync(join(tmpdir(), "koushi-synapse-launch-"));
  const previousPath = process.env.PATH;
  try {
    const capture = join(directory, "argv.json");
    const docker = join(directory, "docker");
    writeFileSync(docker, `#!${process.execPath}\nimport { writeFileSync } from 'node:fs';\nif (process.argv[2] === 'run') writeFileSync(${JSON.stringify(capture)}, JSON.stringify(process.argv.slice(2)));\n`);
    chmodSync(docker, 0o700);
    process.env.PATH = `${directory}:${previousPath}`;
    const child = fixture.startHomeserver("synapse", join(directory, "config"), join(directory, "log"), {
      serverName: "ownership.localhost", port: 18008, dataDir: join(directory, "data")
    });
    const [status] = await once(child, "exit");
    assert.equal(status, 0);
    const argv = JSON.parse(readFileSync(capture, "utf8"));
    const uid = process.getuid?.() ?? 991;
    const gid = process.getgid?.() ?? 991;
    assert.equal(argv[argv.indexOf("--user") + 1], `${uid}:${gid}`);
    assert.equal(argv.includes("UID=0"), uid === 0);
    assert.equal(argv.some((argument) => /^UID=/.test(argument)), uid === 0);
  } finally {
    process.env.PATH = previousPath;
    rmSync(directory, { recursive: true, force: true });
  }
});

// Opt in because this builds the pinned image and uses a real rootful Docker daemon.
// The bind source lives in Docker's Linux filesystem, bypassing Desktop's host
// ownership mapping, so UID assertions exercise Linux permissions even on macOS.
test("pinned Synapse retains non-root editable Linux bind data after success, interruption and startup failure", {
  skip: process.env.KOUSHI_TEST_SYNAPSE_OWNERSHIP !== "1",
  timeout: 120_000
}, () => {
  const directory = mkdtempSync(join(tmpdir(), "koushi-synapse-bind-"));
  const suffix = `${process.pid}-${Date.now()}`;
  const volume = `koushi-synapse-ownership-${suffix}`;
  const image = `koushi-synapse-ownership:${suffix}`;
  const container = `koushi-synapse-ownership-${suffix}`;
  const upstream = "docker.io/matrixdotorg/synapse:v1.157.0";
  const identity = fixture.synapseRuntimeIdentity({ uid: 12345, gid: 23456 });
  function docker(args, { success = true } = {}) {
    const result = spawnSync("docker", args, { encoding: "utf8", timeout: 60_000 });
    if (success) assert.equal(result.status, 0, `docker ${args[0]} failed (status ${result.status})`);
    return result;
  }
  try {
    const daemon = JSON.parse(docker(["info", "--format", "{{json .}}"]).stdout);
    assert.equal(daemon.OSType, "linux", "ownership integration requires a Linux daemon");
    assert.ok(!daemon.SecurityOptions.some((option) => option.includes("rootless")),
      "ownership integration requires rootful Docker");
    writeFileSync(join(directory, "Dockerfile"), fixture.synapseDockerfile());
    writeFileSync(join(directory, "synapse-local-qa-start.sh"), fixture.synapseEntrypoint());
    docker(["build", "-q", "-t", image, directory]);
    docker(["volume", "create", volume]);
    const mountpoint = docker(["volume", "inspect", "--format", "{{.Mountpoint}}", volume]).stdout.trim();
    const mount = ["--mount", `type=bind,source=${mountpoint},target=/fixture`];
    docker(["run", "--rm", ...mount, "--entrypoint", "python3", upstream, "-c",
      "import os; os.chown('/fixture', 12345, 23456); os.chmod('/fixture', 0o700); os.mkdir('/fixture/data', 0o700); os.chown('/fixture/data', 12345, 23456)"]);
    const dataMount = ["--mount", `type=bind,source=${mountpoint}/data,target=/data`];
    const runtime = ["run", "--name", container, ...identity, ...dataMount,
      "-e", "SYNAPSE_SERVER_NAME=ownership.localhost", "-e", "SYNAPSE_REPORT_STATS=no", "-e", "SYNAPSE_NO_TLS=1"];
    docker([...runtime, "-d", image]);
    // Poll the actual Matrix readiness response inside the container, not logs.
    docker(["exec", container, "python3", "-c", `import time, urllib.request
for attempt in range(100):
    try:
        response = urllib.request.urlopen('http://127.0.0.1:8008/_matrix/client/versions', timeout=1)
        assert response.status == 200
        break
    except (OSError, AssertionError):
        time.sleep(0.1)
else:
    raise RuntimeError('Synapse readiness failed')`]);
    docker(["stop", "--time", "5", container]);
    docker(["rm", container]);
    const inspectData = `import os, pathlib
root = pathlib.Path('/data')
assert (root / 'homeserver.db').is_file()
for path in [root, *root.rglob('*')]:
    info = path.stat()
    assert (info.st_uid, info.st_gid) == (12345, 23456), 'fixture ownership changed'
    assert info.st_mode & 2 == 0, 'world-writable fixture'
    if path.is_file():
        with path.open('rb+') as stream: pass
with (root / 'homeserver.yaml').open('a') as stream: stream.write('\\n# host edit\\n')`;
    docker(["run", "--rm", ...identity, ...dataMount, "--entrypoint", "python3", image, "-c", inspectData]);
    // Reuse retained config, then interrupt the real running service.
    docker([...runtime, "-d", image]);
    docker(["kill", container]);
    docker(["rm", "-f", container]);
    docker(["run", "--rm", ...identity, ...dataMount, "--entrypoint", "python3", image, "-c", inspectData]);
    docker(["run", "--rm", ...identity, ...dataMount, "--entrypoint", "python3", image, "-c",
      "from pathlib import Path; Path('/data/homeserver.yaml').write_text('experimental_features: []\\n')"]);
    const failed = docker([...runtime, image], { success: false });
    assert.equal(failed.status, 1, "invalid retained config must fail startup without timing out");
    docker(["rm", container]);
    docker(["run", "--rm", ...identity, ...dataMount, "--entrypoint", "python3", image, "-c", inspectData]);
    // Every retained file and directory is removable by the same non-root host identity.
    docker(["run", "--rm", ...identity, ...mount, "--entrypoint", "python3", image, "-c",
      "import shutil; shutil.rmtree('/fixture/data')"]);
    // A POSIX root invocation must also preserve its own identity rather than
    // trigger upstream's implicit 991 fallback.
    docker(["run", "--rm", ...mount, "--entrypoint", "python3", upstream, "-c",
      "import os; os.mkdir('/fixture/root-data', 0o700)"]);
    const rootMount = ["--mount", `type=bind,source=${mountpoint}/root-data,target=/data`];
    docker(["run", "--rm", ...fixture.synapseRuntimeIdentity({ uid: 0, gid: 23456 }), ...rootMount,
      "-e", "SYNAPSE_SERVER_NAME=ownership.localhost", "-e", "SYNAPSE_REPORT_STATS=no", "-e", "SYNAPSE_NO_TLS=1",
      "--entrypoint", "/start.py", image, "migrate_config"]);
    docker(["run", "--rm", ...rootMount, "--entrypoint", "python3", image, "-c", `from pathlib import Path
root = Path('/data')
assert (root / 'homeserver.yaml').is_file()
for path in [root, *root.rglob('*')]:
    info = path.stat()
    assert (info.st_uid, info.st_gid) == (0, 23456), 'root fixture ownership changed'`]);
  } finally {
    docker(["rm", "-f", container], { success: false });
    docker(["volume", "rm", volume], { success: false });
    docker(["image", "rm", image], { success: false });
    rmSync(directory, { recursive: true, force: true });
  }
});
