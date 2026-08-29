#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
export const repositoryRoot = path.resolve(scriptDirectory, "..");
export const INLINE_TEST_MODULE_LIMIT = 200;
export const FIRST_PARTY_ROOTS = ["crates", "apps/desktop/src-tauri"];
export const ALLOWED_NON_RUST_TARGETS = new Set([
  "docs/architecture/state-machine.md",
  "apps/desktop/src-tauri/capabilities/windows-overlay.json",
  "apps/desktop/src/domain/coreEvents.generated.json"
]);

const identifierStart = (character) => /[A-Za-z_]/u.test(character ?? "");
const identifierPart = (character) => /[A-Za-z0-9_]/u.test(character ?? "");
const openingDelimiters = new Set(["(", "[", "{"]);
const closingDelimiters = new Set([")", "]", "}"]);
const matchingDelimiter = { "(": ")", "[": "]", "{": "}" };
const isOpeningDelimiter = (token) => token?.kind === "punctuation" && openingDelimiters.has(token.value);
const isClosingDelimiter = (token) => token?.kind === "punctuation" && closingDelimiters.has(token.value);

function decodeRustString(value) {
  return value.replace(/\\([\\"'nrt0])/gu, (_, escape) => ({
    "\\": "\\",
    '"': '"',
    "'": "'",
    n: "\n",
    r: "\r",
    t: "\t",
    0: "\0"
  })[escape]);
}

function readQuoted(source, start, prefixLength = 0) {
  const quote = start + prefixLength;
  let escaped = false;
  for (let index = quote + 1; index < source.length; index += 1) {
    const character = source[index];
    if (escaped) {
      escaped = false;
    } else if (character === "\\") {
      escaped = true;
    } else if (character === '"') {
      return {
        end: index + 1,
        value: decodeRustString(source.slice(quote + 1, index))
      };
    }
  }
  return { end: source.length, value: decodeRustString(source.slice(quote + 1)) };
}

function readRawString(source, start) {
  let quote = start;
  if (source[quote] === "b") quote += 1;
  if (source[quote] !== "r") return null;
  quote += 1;
  let hashes = 0;
  while (source[quote + hashes] === "#") hashes += 1;
  quote += hashes;
  if (source[quote] !== '"') return null;
  const terminator = `"${"#".repeat(hashes)}`;
  const close = source.indexOf(terminator, quote + 1);
  const end = close < 0 ? source.length : close + terminator.length;
  return {
    end,
    value: source.slice(quote + 1, close < 0 ? source.length : close)
  };
}

function readChar(source, start) {
  let escaped = false;
  for (let index = start + 1; index < source.length; index += 1) {
    const character = source[index];
    if (escaped) {
      escaped = false;
    } else if (character === "\\") {
      escaped = true;
    } else if (character === "'") {
      return index + 1;
    }
  }
  return -1;
}

/** Lex only enough Rust to safely skip literals/comments and balance items. */
export function lexRust(source) {
  const tokens = [];
  let index = 0;
  let line = 1;

  const advance = (end) => {
    for (; index < end; index += 1) {
      if (source[index] === "\n") line += 1;
    }
  };
  const add = (kind, value, start, end, startLine = line) => {
    tokens.push({ kind, value, start, end, line: startLine });
    advance(end);
  };

  while (index < source.length) {
    const character = source[index];
    const next = source[index + 1];
    if (/\s/u.test(character)) {
      advance(index + 1);
      continue;
    }
    if (character === "/" && next === "/") {
      const end = source.indexOf("\n", index + 2);
      advance(end < 0 ? source.length : end);
      continue;
    }
    if (character === "/" && next === "*") {
      const startLine = line;
      let depth = 1;
      let cursor = index + 2;
      while (cursor < source.length && depth > 0) {
        if (source[cursor] === "/" && source[cursor + 1] === "*") {
          depth += 1;
          cursor += 2;
        } else if (source[cursor] === "*" && source[cursor + 1] === "/") {
          depth -= 1;
          cursor += 2;
        } else {
          cursor += 1;
        }
      }
      add("comment", "", index, cursor, startLine);
      continue;
    }

    const raw = readRawString(source, index);
    if (raw) {
      add("string", raw.value, index, raw.end);
      continue;
    }
    if (character === '"') {
      const quoted = readQuoted(source, index);
      add("string", quoted.value, index, quoted.end);
      continue;
    }
    if (character === "b" && next === '"') {
      const quoted = readQuoted(source, index, 1);
      add("string", quoted.value, index, quoted.end);
      continue;
    }
    if (character === "b" && next === "'") {
      const end = readChar(source, index + 1);
      if (end > 0) {
        add("char", "", index, end);
        continue;
      }
    }
    if (character === "'") {
      const isSimpleChar = !identifierStart(next) || source[index + 2] === "'";
      const end = isSimpleChar ? readChar(source, index) : -1;
      if (end > 0) {
        add("char", "", index, end);
        continue;
      }
      if (identifierStart(next)) {
        let end = index + 2;
        while (identifierPart(source[end])) end += 1;
        add("lifetime", source.slice(index + 1, end), index, end);
        continue;
      }
    }
    if (identifierStart(character)) {
      let end = index + 1;
      while (identifierPart(source[end])) end += 1;
      add("identifier", source.slice(index, end), index, end);
      continue;
    }
    if (/[0-9]/u.test(character)) {
      let end = index + 1;
      while (/[A-Za-z0-9_\.]/u.test(source[end] ?? "")) end += 1;
      add("number", source.slice(index, end), index, end);
      continue;
    }
    add("punctuation", character, index, index + 1);
  }
  return tokens;
}

function delimiterPairs(tokens) {
  const openToClose = new Map();
  const closeToOpen = new Map();
  const stack = [];
  const errors = [];
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    if (isOpeningDelimiter(token)) {
      stack.push(index);
    } else if (isClosingDelimiter(token)) {
      const open = stack.pop();
      if (open === undefined || matchingDelimiter[tokens[open].value] !== token.value) {
        errors.push({ kind: "unbalanced delimiter", token: tokens[index] });
      } else {
        openToClose.set(open, index);
        closeToOpen.set(index, open);
      }
    }
  }
  for (const open of stack) errors.push({ kind: "unbalanced delimiter", token: tokens[open] });
  return { openToClose, closeToOpen, errors };
}

function braceDepths(tokens) {
  const depths = [];
  let depth = 0;
  for (const token of tokens) {
    depths.push(depth);
    if (token.kind === "punctuation" && token.value === "{") depth += 1;
    if (token.kind === "punctuation" && token.value === "}") depth -= 1;
  }
  return depths;
}

function cfgAttribute(tokens, hashIndex, pairs) {
  if (tokens[hashIndex]?.value !== "#" || tokens[hashIndex + 1]?.value !== "[") return null;
  const close = pairs.openToClose.get(hashIndex + 1);
  if (close === undefined) return null;
  if (tokens[hashIndex + 2]?.value !== "cfg") return null;
  const expression = tokens.slice(hashIndex + 3, close);
  let hasTest = false;
  let negated = false;
  for (let index = 0; index < expression.length; index += 1) {
    const token = expression[index];
    if (token.value === "not" && expression[index + 1]?.value === "(") {
      negated = true;
    } else if (token.value === "test" && !negated) {
      hasTest = true;
    }
    if (token.value === ")") negated = false;
  }
  return { start: tokens[hashIndex].start, end: tokens[close].end, line: tokens[hashIndex].line, hasTest };
}

function attachedAttributes(tokens, moduleIndex, pairs) {
  let cursor = moduleIndex - 1;
  if (tokens[cursor]?.value === ")") {
    const open = pairs.closeToOpen.get(cursor);
    if (open !== undefined && tokens[open - 1]?.value === "pub") cursor = open - 2;
  }
  while (tokens[cursor]?.value === "pub" || tokens[cursor]?.value === "unsafe") cursor -= 1;

  const attributes = [];
  while (tokens[cursor]?.value === "]") {
    const open = pairs.closeToOpen.get(cursor);
    if (open === undefined || tokens[open - 1]?.value !== "#") break;
    const attribute = cfgAttribute(tokens, open - 1, pairs);
    if (attribute) attributes.unshift(attribute);
    cursor = open - 2;
  }
  return attributes;
}

function moduleInventory(source, fileName) {
  const tokens = lexRust(source);
  const pairs = delimiterPairs(tokens);
  const depths = braceDepths(tokens);
  const inline = [];
  const external = [];
  const nested = [];
  const errors = pairs.errors.map(({ kind, token }) => `${fileName}:${token.line}:${kind}`);

  for (let index = 0; index < tokens.length; index += 1) {
    if (tokens[index].value !== "mod" || tokens[index + 1]?.kind !== "identifier") continue;
    const nameToken = tokens[index + 1];
    const attributes = attachedAttributes(tokens, index, pairs);
    if (!attributes.some(({ hasTest }) => hasTest)) continue;
    const declaration = tokens[index + 2];
    const module = {
      file: fileName,
      name: nameToken.value,
      line: attributes[0]?.line ?? tokens[index].line,
      start: attributes[0]?.start ?? tokens[index].start,
      declarationLine: tokens[index].line,
      physicalLines: null,
      overThreshold: false
    };
    if (declaration?.value === ";") {
      external.push(module);
      continue;
    }
    if (declaration?.value !== "{") {
      errors.push(`${fileName}:${tokens[index].line}:ambiguous cfg(test) module`);
      continue;
    }
    const close = pairs.openToClose.get(index + 2);
    if (close === undefined) {
      errors.push(`${fileName}:${tokens[index].line}:unclosed cfg(test) module`);
      continue;
    }
    module.end = tokens[close].end;
    module.physicalLines = source.slice(module.start, module.end).split("\n").length;
    module.overThreshold = module.physicalLines >= INLINE_TEST_MODULE_LIMIT;
    if (depths[index] > 0) {
      nested.push(module);
    } else {
      inline.push(module);
    }
  }
  return { inline, external, nested, errors };
}

export function findInlineTestModules(source, fileName = "fixture.rs") {
  return moduleInventory(source, fileName).inline;
}

function splitArguments(tokens) {
  const argumentsList = [];
  let current = [];
  let depth = 0;
  for (const token of tokens) {
    if (isOpeningDelimiter(token)) depth += 1;
    if (isClosingDelimiter(token)) depth -= 1;
    if (token.value === "," && depth === 0) {
      argumentsList.push(current);
      current = [];
    } else {
      current.push(token);
    }
  }
  if (current.length > 0) argumentsList.push(current);
  return argumentsList;
}

function evaluateExpression(tokens, manifestDir) {
  if (tokens.length === 1 && tokens[0].kind === "string") return tokens[0].value;
  if (tokens[0]?.kind !== "identifier" || tokens[1]?.value !== "!" || tokens[2]?.value !== "(") return null;
  let depth = 0;
  let close = -1;
  for (let index = 2; index < tokens.length; index += 1) {
    if (tokens[index].kind === "punctuation" && tokens[index].value === "(") depth += 1;
    if (tokens[index].kind === "punctuation" && tokens[index].value === ")") {
      depth -= 1;
      if (depth === 0) {
        close = index;
        break;
      }
    }
  }
  if (close !== tokens.length - 1) return null;
  const args = splitArguments(tokens.slice(3, close));
  if (tokens[0].value === "env") {
    return evaluateExpression(args[0] ?? [], manifestDir) === "CARGO_MANIFEST_DIR" ? manifestDir : null;
  }
  if (tokens[0].value === "concat") {
    const values = args.map((argument) => evaluateExpression(argument, manifestDir));
    return values.every((value) => value !== null) ? values.join("") : null;
  }
  return null;
}

function findManifestDir(filePath, root) {
  let directory = path.dirname(filePath);
  const rootPath = path.resolve(root);
  while (directory.startsWith(rootPath)) {
    if (fs.existsSync(path.join(directory, "Cargo.toml"))) return directory;
    const parent = path.dirname(directory);
    if (parent === directory) break;
    directory = parent;
  }
  return rootPath;
}

function displayPath(absolutePath, root) {
  const relative = path.relative(root, absolutePath).split(path.sep).join("/");
  return relative && !relative.startsWith("../") && relative !== ".." ? relative : "<outside-repository>";
}

function normalizeFilePath(filePath, root) {
  return path.isAbsolute(filePath) ? path.normalize(filePath) : path.resolve(root, filePath);
}

export function findIncludeStrInvocations(source, filePath, options = {}) {
  const root = path.resolve(options.repositoryRoot ?? repositoryRoot);
  const absoluteFile = normalizeFilePath(filePath, root);
  const tokens = lexRust(source).map((token, index) => ({ ...token, index }));
  const pairs = delimiterPairs(tokens);
  const manifestDir = findManifestDir(absoluteFile, root);
  const includes = [];

  for (let index = 0; index < tokens.length; index += 1) {
    if (tokens[index].value !== "include_str" || tokens[index + 1]?.value !== "!" || tokens[index + 2]?.value !== "(") continue;
    const close = pairs.openToClose.get(index + 2);
    if (close === undefined) continue;
    const argumentTokens = tokens.slice(index + 3, close).filter(({ kind }) => kind !== "comment");
    const expression = evaluateExpression(argumentTokens, manifestDir);
    const targetPath = expression === null
      ? null
      : path.isAbsolute(expression)
        ? path.normalize(expression)
        : path.resolve(path.dirname(absoluteFile), expression);
    const target = targetPath ? displayPath(targetPath, root) : "<unresolved>";
    const exists = targetPath !== null && fs.existsSync(targetPath);
    includes.push({
      file: displayPath(absoluteFile, root),
      line: tokens[index].line,
      target,
      exists,
      rustSource: target.endsWith(".rs"),
      allowedNonRust: ALLOWED_NON_RUST_TARGETS.has(target),
      resolvedPath: targetPath
    });
    index = close;
  }
  return includes;
}

export function analyzeRustSource(source, options = {}) {
  const root = path.resolve(options.repositoryRoot ?? repositoryRoot);
  const fileName = displayPath(normalizeFilePath(options.filePath ?? "fixture.rs", root), root);
  const modules = moduleInventory(source, fileName);
  const includes = findIncludeStrInvocations(source, options.filePath ?? "fixture.rs", { ...options, repositoryRoot: root });
  const rustSourceIncludes = includes.filter(({ rustSource }) => rustSource);
  const nonRustArtifacts = includes.filter(({ allowedNonRust }) => allowedNonRust);
  const unexpectedArtifacts = includes.filter(({ target, rustSource, allowedNonRust }) => target !== "<unresolved>" && !rustSource && !allowedNonRust);
  const violations = [...modules.errors.map((message) => ({ kind: "parse", message }))];
  for (const module of modules.nested) {
    violations.push({ kind: "nested-module", file: module.file, line: module.line, name: module.name });
  }
  for (const module of modules.inline.filter(({ overThreshold }) => overThreshold)) {
    violations.push({ kind: "inline-module", file: module.file, line: module.line, name: module.name, physicalLines: module.physicalLines });
  }
  for (const include of rustSourceIncludes) violations.push({ kind: "rust-source-include", ...include });
  for (const include of unexpectedArtifacts) violations.push({ kind: "unexpected-include", ...include });
  for (const include of includes.filter(({ exists }) => !exists)) violations.push({ kind: "unresolved-include", ...include });
  return {
    ...modules,
    inlineTestModules: modules.inline,
    externalTestModules: modules.external,
    nestedTestModules: modules.nested,
    includes,
    rustSourceIncludes,
    nonRustArtifacts,
    unexpectedArtifacts,
    violations
  };
}

function rustFiles(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) return rustFiles(entryPath);
    return entry.isFile() && entry.name.endsWith(".rs") ? [entryPath] : [];
  }).sort();
}

export function scanRepository(root = repositoryRoot) {
  const repository = path.resolve(root);
  const files = FIRST_PARTY_ROOTS.flatMap((relativeRoot) => rustFiles(path.join(repository, relativeRoot))).sort();
  const analyses = files.map((filePath) => analyzeRustSource(fs.readFileSync(filePath, "utf8"), {
    filePath,
    repositoryRoot: repository
  }));
  const result = {
    rustFileCount: files.length,
    files,
    analyses,
    inlineTestModules: analyses.flatMap(({ inlineTestModules }) => inlineTestModules),
    externalTestModules: analyses.flatMap(({ externalTestModules }) => externalTestModules),
    nestedTestModules: analyses.flatMap(({ nestedTestModules }) => nestedTestModules),
    includes: analyses.flatMap(({ includes }) => includes),
    rustSourceIncludes: analyses.flatMap(({ rustSourceIncludes }) => rustSourceIncludes),
    nonRustArtifacts: analyses.flatMap(({ nonRustArtifacts }) => nonRustArtifacts),
    unexpectedArtifacts: analyses.flatMap(({ unexpectedArtifacts }) => unexpectedArtifacts),
    violations: analyses.flatMap(({ violations }) => violations)
  };
  return result;
}

export function formatViolation(violation) {
  if (typeof violation === "string") return violation;
  if (violation.kind === "parse") return violation.message;
  if (violation.kind === "nested-module") return `${violation.file}:${violation.line}:nested inline cfg(test) module ${violation.name}`;
  if (violation.kind === "inline-module") return `${violation.file}:${violation.line}:inline cfg(test) module ${violation.name} has ${violation.physicalLines} physical lines (limit ${INLINE_TEST_MODULE_LIMIT})`;
  if (violation.kind === "rust-source-include") return `${violation.file}:${violation.line}:include_str! targets Rust source ${violation.target}`;
  if (violation.kind === "unexpected-include") return `${violation.file}:${violation.line}:include_str! targets unapproved artifact ${violation.target}`;
  if (violation.kind === "unresolved-include") return `${violation.file}:${violation.line}:include_str! target could not be resolved`;
  return "Rust test structure violation";
}

function groupedTargets(includes) {
  const counts = new Map();
  for (const include of includes) counts.set(include.target, (counts.get(include.target) ?? 0) + 1);
  return [...counts.entries()].sort(([left], [right]) => left.localeCompare(right));
}

export function inventoryReport(result) {
  const threshold = result.inlineTestModules.filter(({ overThreshold }) => overThreshold);
  const lines = [
    "Rust test structure inventory (transition mode)",
    `Rust files: ${result.rustFileCount}`,
    `include_str! invocations: ${result.includes.length}`,
    `Rust-source include invocations: ${result.rustSourceIncludes.length}`,
    `Non-Rust artifact invocations: ${result.nonRustArtifacts.length}`,
    `Inline cfg(test) modules: ${result.inlineTestModules.length}`,
    `Inline cfg(test) modules at/over ${INLINE_TEST_MODULE_LIMIT} lines: ${threshold.length}`,
    `External/path cfg(test) modules: ${result.externalTestModules.length}`,
    `Nested cfg(test) modules rejected from top-level inventory: ${result.nestedTestModules.length}`,
    "Include targets:"
  ];
  for (const [target, count] of groupedTargets(result.includes)) lines.push(`- ${target}: ${count}`);
  lines.push("Allowed non-Rust artifacts:");
  for (const [target, count] of groupedTargets(result.nonRustArtifacts)) lines.push(`- ${target}: ${count}`);
  lines.push(`Threshold list (${threshold.length}):`);
  for (const module of threshold.sort((left, right) => `${left.file}:${left.line}`.localeCompare(`${right.file}:${right.line}`))) {
    lines.push(`- ${module.file}:${module.line}:${module.name}: ${module.physicalLines} lines`);
  }
  return `${lines.join("\n")}\n`;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const inventory = process.argv.includes("--inventory");
  const result = scanRepository();
  if (inventory) {
    process.stdout.write(inventoryReport(result));
  } else if (result.violations.length > 0) {
    console.error("Rust test structure violations:");
    for (const violation of result.violations) console.error(`- ${formatViolation(violation)}`);
    process.exitCode = 1;
  }
}
