import { execFileSync } from "node:child_process";

export const repoRoot = new URL("../../../../", import.meta.url).pathname;

export function runScript(script: string, args: string[] = []): string {
  return execFileSync(process.execPath, [script, ...args], { cwd: repoRoot, encoding: "utf8" });
}

export function gitTrackedFiles(): string[] {
  return execFileSync("git", ["ls-files"], { cwd: repoRoot, encoding: "utf8" })
    .split("\n").map((file) => file.trim()).filter(Boolean);
}
