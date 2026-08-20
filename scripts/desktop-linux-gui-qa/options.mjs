import { createRequire } from "node:module";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const desktopDir = join(repoRoot, "apps", "desktop");
const desktopPackageRequire = createRequire(new URL("../../apps/desktop/package.json", import.meta.url));
const args = new Set(process.argv.slice(2));

function optionValue(name) {
  const prefix = `${name}=`;
  const inline = process.argv.find((argument) => argument.startsWith(prefix));
  if (inline) return inline.slice(prefix.length);
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function resolveArtifactRoot(artifactDirOption) {
  if (!artifactDirOption) return join(repoRoot, "artifacts", "linux-gui-qa");
  return isAbsolute(artifactDirOption) ? artifactDirOption : resolve(repoRoot, artifactDirOption);
}

const guiScenario = optionValue("--scenario") ?? "signed-out";
const serverOption = optionValue("--server") ?? "tuwunel";
const qaProfile = optionValue("--qa-profile");
const realLoginFromStdin = args.has("--real-login-from-stdin");
const allowEmptyTimeline = args.has("--allow-empty-timeline");
const artifactRoot = resolveArtifactRoot(optionValue("--artifact-dir"));
const timeoutMs = Number(optionValue("--timeout-ms") ?? "120000");

export {
  allowEmptyTimeline,
  args,
  artifactRoot,
  desktopDir,
  desktopPackageRequire,
  guiScenario,
  optionValue,
  qaProfile,
  realLoginFromStdin,
  repoRoot,
  resolveArtifactRoot,
  serverOption,
  timeoutMs
};
