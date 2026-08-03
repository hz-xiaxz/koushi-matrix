import { writeFileSync } from "node:fs";
import { join } from "node:path";

export function writeValidatedQaOutputFiles({
  directory,
  label,
  stdout = "",
  stderr = "",
  validate
}) {
  const output = { stdout: stdout || "", stderr: stderr || "" };
  validate(output);
  writeFileSync(join(directory, `${label}-stdout.log`), output.stdout);
  writeFileSync(join(directory, `${label}-stderr.log`), output.stderr);
}
