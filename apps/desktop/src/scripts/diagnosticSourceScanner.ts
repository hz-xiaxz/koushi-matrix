import { readdirSync,readFileSync } from "node:fs";
import { relative,sep } from "node:path";
import { repoRoot } from "./releaseTestSupport";

export type DiagnosticSource = {
  relativePath: string;
  source: string;
};

export type DiagnosticGateFinding = {
  relativePath: string;
  line: number;
  location: string;
  reason: string;
};

export type SourceScope = {
  name: string;
  start: number;
  end: number;
};

export const DIAGNOSTIC_ENV_PATTERN = /KOUSHI_[A-Z0-9_]*(?:TRACE|DIAGNOST)/;
export const TEST_ATTRIBUTE_PATTERN = /^\s*#\[(?:(?:tokio|async_std)::)?test\]\s*$/;
export const SYNTHETIC_TRACE_ENV = ["KOUSHI", "SYNTH_TRACE"].join("_");
export const SYNTHETIC_TRACE_DECLARATION = `const SYNTHETIC_TRACE_ENV: &str = "${SYNTHETIC_TRACE_ENV}";`;
export const GATED_DIAGNOSTIC_REASON =
  "env-gated diagnostic producer has no always-on structured collection";
export const REMOVED_DIAGNOSTIC_ENV_LITERALS = [
  "KOUSHI_STARTUP_TRACE",
  "KOUSHI_SUBSCRIBE_TRACE",
  "KOUSHI_TIMELINE_ITEM_TRACE",
  "KOUSHI_UNREAD_TRACE",
  "KOUSHI_SEARCH_TRACE",
  "KOUSHI_SYNC_TRACE",
  "KOUSHI_CORE_ACTOR_TRACE",
  "KOUSHI_DEBUG_SDK_ERROR"
];

export function runtimeRustSources(): DiagnosticSource[] {
  const roots = ["crates/koushi-sdk/src", "crates/koushi-core/src", "apps/desktop/src-tauri/src"];
  const sources: DiagnosticSource[] = [];

  function visit(directory: string): void {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const file = `${directory}/${entry.name}`;
      const fileParts = relative(repoRoot, file).split(sep);
      if (fileParts.some((part) => ["bin", "build", "generated", "target"].includes(part))) {
        continue;
      }
      if (entry.isDirectory()) {
        visit(file);
      } else if (entry.isFile() && file.endsWith(".rs")) {
        sources.push({
          relativePath: fileParts.join("/"),
          source: readFileSync(file, "utf8")
        });
      }
    }
  }

  for (const root of roots) {
    visit(`${repoRoot}${root}`);
  }
  return sources;
}

export function productionRustLines(source: string): string[] {
  const lines = source.split("\n");
  const productionLines = [...lines];

  for (let index = 0; index < lines.length; index += 1) {
    if (!isTestOnlyAttribute(lines[index])) {
      continue;
    }

    let itemStart = index + 1;
    while (itemStart < lines.length && lines[itemStart].trim() === "") {
      itemStart += 1;
    }
    const itemEnd = itemStart < lines.length ? testOnlyItemEnd(lines, itemStart) : index;
    for (let itemIndex = index; itemIndex <= itemEnd; itemIndex += 1) {
      productionLines[itemIndex] = "";
    }
    index = itemEnd;
  }

  return productionLines;
}

export function isTestOnlyAttribute(line: string): boolean {
  if (TEST_ATTRIBUTE_PATTERN.test(line)) {
    return true;
  }
  const match = /^\s*#\[cfg\((.*)\)\]\s*$/.exec(line);
  return match ? isTestOnlyCfgExpression(match[1]) : false;
}

export function isTestOnlyCfgExpression(expression: string): boolean {
  const trimmed = expression.trim();
  if (trimmed === "test") {
    return true;
  }

  const open = trimmed.indexOf("(");
  if (open <= 0 || !trimmed.endsWith(")")) {
    return false;
  }

  const name = trimmed.slice(0, open).trim();
  const argumentsText = trimmed.slice(open + 1, -1);
  const argumentsList = splitCfgArguments(argumentsText);
  if (argumentsList === null || argumentsList.length === 0) {
    return false;
  }

  if (name === "all") {
    return argumentsList.some((argument) => isTestOnlyCfgExpression(argument));
  }
  if (name === "any") {
    return argumentsList.every((argument) => isTestOnlyCfgExpression(argument));
  }
  return false;
}

export function splitCfgArguments(expression: string): string[] | null {
  const argumentsList: string[] = [];
  let start = 0;
  let depth = 0;
  let inString = false;
  let escaped = false;

  for (let index = 0; index < expression.length; index += 1) {
    const character = expression[index];
    if (inString) {
      if (escaped) {
        escaped = false;
      } else if (character === "\\") {
        escaped = true;
      } else if (character === '"') {
        inString = false;
      }
      continue;
    }
    if (character === '"') {
      inString = true;
    } else if (character === "(") {
      depth += 1;
    } else if (character === ")") {
      depth -= 1;
      if (depth < 0) {
        return null;
      }
    } else if (character === "," && depth === 0) {
      argumentsList.push(expression.slice(start, index).trim());
      start = index + 1;
    }
  }

  if (inString || depth !== 0) {
    return null;
  }
  const last = expression.slice(start).trim();
  if (last.length > 0) {
    argumentsList.push(last);
  }
  return argumentsList;
}

export function testOnlyItemEnd(lines: readonly string[], start: number): number {
  let depth = 0;
  let opened = false;
  for (let index = start; index < lines.length; index += 1) {
    const structural = structuralRustLine(lines[index]);
    const delta = braceDelta(lines[index]);
    if (delta > 0) {
      opened = true;
    }
    depth += delta;
    if (opened && depth <= 0) {
      return index;
    }
    if (!opened && (structural.includes(";") || structural.trimEnd().endsWith(","))) {
      return index;
    }
  }
  return lines.length - 1;
}

export type RustLexicalView = {
  code: string;
  stringValues: string[];
  stringSpans: Array<{ value: string; start: number; end: number }>;
};

export function lexicalRustView(source: string): RustLexicalView {
  const code = [...source];
  const stringValues: string[] = [];
  const stringSpans: Array<{ value: string; start: number; end: number }> = [];

  function blank(start: number, endExclusive: number): void {
    for (let index = start; index < endExclusive; index += 1) {
      if (code[index] !== "\n" && code[index] !== "\r") {
        code[index] = " ";
      }
    }
  }

  function rawStringAt(start: number): { contentStart: number; hashes: number } | null {
    const previous = source[start - 1];
    if (previous && /[A-Za-z0-9_]/.test(previous)) {
      return null;
    }
    let cursor = start;
    if (source[cursor] === "b") {
      cursor += 1;
    }
    if (source[cursor] !== "r") {
      return null;
    }
    cursor += 1;
    let hashes = 0;
    while (source[cursor] === "#") {
      hashes += 1;
      cursor += 1;
    }
    return source[cursor] === '"' ? { contentStart: cursor + 1, hashes } : null;
  }

  let index = 0;
  while (index < source.length) {
    if (source[index] === "/" && source[index + 1] === "/") {
      const start = index;
      while (index < source.length && source[index] !== "\n") {
        index += 1;
      }
      blank(start, index);
      continue;
    }
    if (source[index] === "/" && source[index + 1] === "*") {
      const start = index;
      let depth = 1;
      index += 2;
      while (index < source.length && depth > 0) {
        if (source[index] === "/" && source[index + 1] === "*") {
          depth += 1;
          index += 2;
        } else if (source[index] === "*" && source[index + 1] === "/") {
          depth -= 1;
          index += 2;
        } else {
          index += 1;
        }
      }
      blank(start, index);
      continue;
    }

    const rawString = rawStringAt(index);
    if (rawString) {
      const start = index;
      const terminator = `"${"#".repeat(rawString.hashes)}`;
      const end = source.indexOf(terminator, rawString.contentStart);
      const contentEnd = end === -1 ? source.length : end;
      const value = source.slice(rawString.contentStart, contentEnd);
      stringValues.push(value);
      index = end === -1 ? source.length : end + terminator.length;
      stringSpans.push({ value, start, end: index });
      blank(start, index);
      continue;
    }

    if (source[index] === '"') {
      const start = index;
      let value = "";
      index += 1;
      let escaped = false;
      while (index < source.length) {
        const character = source[index];
        if (!escaped && character === '"') {
          index += 1;
          break;
        }
        value += character;
        if (escaped) {
          escaped = false;
        } else if (character === "\\") {
          escaped = true;
        }
        index += 1;
      }
      stringValues.push(value);
      stringSpans.push({ value, start, end: index });
      blank(start, index);
      continue;
    }

    index += 1;
  }

  return { code: code.join(""), stringValues, stringSpans };
}

export function structuralRustLine(line: string): string {
  return lexicalRustView(line).code;
}

export function braceDelta(line: string): number {
  return [...structuralRustLine(line)].reduce(
    (delta, character) => delta + (character === "{" ? 1 : character === "}" ? -1 : 0),
    0
  );
}

export function braceDepths(lines: readonly string[]): number[] {
  let depth = 0;
  return lines.map((line) => {
    const lineDepth = depth;
    depth += braceDelta(line);
    return lineDepth;
  });
}

export function previousStatementRange(
  lines: readonly string[],
  endExclusive: number,
  parentDepth: number,
  depths: readonly number[],
  minimumStart: number
): [number, number] | null {
  let end = endExclusive - 1;
  while (end >= 0 && structuralRustLine(lines[end]).trim() === "") {
    end -= 1;
  }
  if (end < minimumStart || depths[end] < parentDepth) {
    return null;
  }

  const endStructural = structuralRustLine(lines[end]);
  if (endStructural.includes("}") && !endStructural.includes(";")) {
    const blockStart = matchingBlockStart(lines, end, minimumStart);
    if (blockStart === null) {
      return null;
    }
    for (let index = blockStart; index >= minimumStart; index -= 1) {
      if (
        depths[index] === parentDepth &&
        /^(?:if|for|match|while|loop)\b/.test(structuralRustLine(lines[index]).trim())
      ) {
        return [index, end];
      }
    }
    return [blockStart, end];
  }

  for (let index = end - 1; index >= minimumStart; index -= 1) {
    if (
      depths[index] + braceDelta(lines[index]) === parentDepth &&
      structuralRustLine(lines[index]).trimEnd().endsWith("}")
    ) {
      return [index + 1, end];
    }
    if (depths[index] === parentDepth && structuralRustLine(lines[index]).includes(";")) {
      return [index + 1, end];
    }
  }
  return [minimumStart, end];
}

export function matchingBlockStart(
  lines: readonly string[],
  end: number,
  minimumStart: number
): number | null {
  let nestedClosures = 0;
  for (let index = end; index >= minimumStart; index -= 1) {
    const structural = structuralRustLine(lines[index]);
    for (let characterIndex = structural.length - 1; characterIndex >= 0; characterIndex -= 1) {
      const character = structural[characterIndex];
      if (character === "}") {
        nestedClosures += 1;
      } else if (character === "{") {
        nestedClosures -= 1;
        if (nestedClosures === 0) {
          return index;
        }
      }
    }
  }
  return null;
}

export function blockEnd(lines: readonly string[], start: number): number {
  let depth = 0;
  let opened = false;
  for (let index = start; index < lines.length; index += 1) {
    const structural = structuralRustLine(lines[index]);
    const delta = braceDelta(structural);
    if (delta > 0) {
      opened = true;
    }
    depth += delta;
    if (opened && depth <= 0) {
      return index;
    }
    if (!opened && balancedBlockEndsLine(structural)) {
      return index;
    }
  }
  return lines.length - 1;
}

export function balancedBlockEndsLine(structural: string): boolean {
  let depth = 0;
  let opened = false;
  for (let index = 0; index < structural.length; index += 1) {
    if (structural[index] === "{") {
      depth += 1;
      opened = true;
    } else if (structural[index] === "}") {
      depth -= 1;
      if (opened && depth === 0 && structural.slice(index + 1).trim() === "") {
        return true;
      }
    }
  }
  return false;
}

export function sourceScopes(lines: readonly string[]): SourceScope[] {
  const scopes: SourceScope[] = [];
  const declarations = [
    /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z0-9_]+)/,
    /^\s*macro_rules!\s+([A-Za-z0-9_]+)/
  ];
  for (let index = 0; index < lines.length; index += 1) {
    const declaration = declarations
      .map((pattern) => pattern.exec(lines[index]))
      .find((match) => match !== null);
    if (declaration) {
      scopes.push({
        name: declaration[1],
        start: index,
        end: blockEnd(lines, index)
      });
    }
  }
  return scopes;
}

export type DiagnosticSourceAnalysis = {
  relativePath: string;
  rawLines: string[];
  codeLines: string[];
  depths: number[];
  scopes: SourceScope[];
  constants: Set<string>;
  moduleQualifiers: string[];
};

export type HelperResolution = {
  localByPath: Map<string, Set<string>>;
  qualified: Set<string>;
};

export function envConstants(rawLines: readonly string[], codeLines: readonly string[]): Set<string> {
  const constants = new Set<string>();
  for (let index = 0; index < codeLines.length; index += 1) {
    const match = /\bconst\s+([A-Z][A-Z0-9_]*)\s*:\s*&str\s*=/.exec(codeLines[index]);
    if (
      match &&
      lexicalRustView(rawLines[index]).stringValues.some((value) =>
        DIAGNOSTIC_ENV_PATTERN.test(value)
      )
    ) {
      constants.add(match[1]);
    }
  }
  return constants;
}

export function directDiagnosticEnvCheck(
  rawText: string,
  codeText: string,
  constants: Set<string>
): boolean {
  if (!/std::env::(?:var_os|var)\s*\(/.test(codeText)) {
    return false;
  }
  if (lexicalRustView(rawText).stringValues.some((value) => DIAGNOSTIC_ENV_PATTERN.test(value))) {
    return true;
  }
  return [...constants].some((constant) => new RegExp(`\\b${constant}\\b`).test(codeText));
}

export function moduleQualifiers(relativePath: string): string[] {
  const parts = relativePath.split("/").filter((part) => part.length > 0);
  if (parts.length === 0) {
    return [];
  }
  parts[parts.length - 1] = parts.at(-1)!.replace(/\.rs$/, "");
  if (parts.at(-1) === "mod") {
    parts.pop();
  }
  const sourceRoot = Math.max(parts.lastIndexOf("src"), parts.lastIndexOf("fixtures"));
  const moduleParts = parts.slice(sourceRoot + 1);
  return moduleParts.map((_, index) => moduleParts.slice(index).join("::"));
}

export function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function normalizedHelperCode(codeText: string): string {
  return codeText.replace(/\b(?:crate|self|super|Self)::/g, (prefix) => " ".repeat(prefix.length));
}

export function normalizedHelperName(name: string): string {
  return name.replace(/^(?:(?:crate|self|super|Self)::)+/, "");
}

export function callArguments(rawText: string, codeText: string, name: string): string[] {
  const argumentsList: string[] = [];
  const normalizedCode = normalizedHelperCode(codeText);
  const pattern = new RegExp(
    `(?:^|[^A-Za-z0-9_:.])${escapeRegExp(normalizedHelperName(name))}\\s*\\(`,
    "g"
  );
  for (const match of normalizedCode.matchAll(pattern)) {
    const open = normalizedCode.indexOf("(", match.index);
    let depth = 0;
    for (let index = open; index < normalizedCode.length; index += 1) {
      if (normalizedCode[index] === "(") {
        depth += 1;
      } else if (normalizedCode[index] === ")") {
        depth -= 1;
        if (depth === 0) {
          argumentsList.push(rawText.slice(open + 1, index));
          break;
        }
      }
    }
  }
  return argumentsList;
}

export function hasNamedCall(codeText: string, name: string): boolean {
  return new RegExp(
    `(?:^|[^A-Za-z0-9_:.])${escapeRegExp(normalizedHelperName(name))}\\s*\\(`
  ).test(normalizedHelperCode(codeText));
}

export function hasHelperCall(codeText: string, helpers: Set<string>, currentScopeName = ""): boolean {
  return [...helpers]
    .filter((name) => name !== currentScopeName)
    .some((name) => hasNamedCall(codeText, name));
}

export function resolveHelpers(
  analyses: readonly DiagnosticSourceAnalysis[],
  directMatch: (analysis: DiagnosticSourceAnalysis, scope: SourceScope) => boolean,
  transitive: boolean,
  wrapperMatch: (analysis: DiagnosticSourceAnalysis, scope: SourceScope) => boolean = () => true
): HelperResolution {
  const localByPath = new Map<string, Set<string>>();
  for (const analysis of analyses) {
    localByPath.set(
      analysis.relativePath,
      new Set(
        analysis.scopes.filter((scope) => directMatch(analysis, scope)).map((scope) => scope.name)
      )
    );
  }

  let changed = transitive;
  while (changed) {
    changed = false;
    const qualified = new Set<string>();
    for (const analysis of analyses) {
      for (const name of localByPath.get(analysis.relativePath) ?? []) {
        for (const qualifier of analysis.moduleQualifiers) {
          qualified.add(`${qualifier}::${name}`);
        }
      }
    }
    for (const analysis of analyses) {
      const local = localByPath.get(analysis.relativePath)!;
      const visible = new Set([...local, ...qualified]);
      for (const scope of analysis.scopes) {
        if (local.has(scope.name) || !wrapperMatch(analysis, scope)) {
          continue;
        }
        const codeText = analysis.codeLines.slice(scope.start, scope.end + 1).join("\n");
        if (hasHelperCall(codeText, visible, scope.name)) {
          local.add(scope.name);
          changed = true;
        }
      }
    }
  }

  const qualified = new Set<string>();
  for (const analysis of analyses) {
    for (const name of localByPath.get(analysis.relativePath) ?? []) {
      for (const qualifier of analysis.moduleQualifiers) {
        qualified.add(`${qualifier}::${name}`);
      }
    }
  }
  return { localByPath, qualified };
}

export function scopeReturnsBool(analysis: DiagnosticSourceAnalysis, scope: SourceScope): boolean {
  const text = analysis.codeLines.slice(scope.start, scope.end + 1).join("\n");
  const openingBrace = text.indexOf("{");
  return /->\s*bool\b/.test(openingBrace === -1 ? text : text.slice(0, openingBrace));
}

export function visibleHelpers(resolution: HelperResolution, relativePath: string): Set<string> {
  const local = resolution.localByPath.get(relativePath) ?? [];
  return new Set([...local, ...[...local].map((name) => `Self::${name}`), ...resolution.qualified]);
}

export function statementEnd(
  codeLines: readonly string[],
  start: number,
  maximumEnd: number,
  depths: readonly number[]
): number {
  const parentDepth = depths[start];
  for (let index = start; index <= maximumEnd; index += 1) {
    if (depths[index] === parentDepth && codeLines[index].includes(";")) {
      return index;
    }
  }
  return start;
}

export function bindingInitializer(rawText: string, codeText: string): { raw: string; code: string } {
  const equals = codeText.indexOf("=");
  const semicolon = codeText.lastIndexOf(";");
  const end = semicolon > equals ? semicolon : codeText.length;
  return equals === -1
    ? { raw: "", code: "" }
    : {
        raw: rawText.slice(equals + 1, end),
        code: codeText.slice(equals + 1, end)
      };
}

export function localEnvironmentAliases(
  analysis: DiagnosticSourceAnalysis,
  scope: SourceScope,
  envHelpers: Set<string>
): Set<string> {
  const aliases = new Set<string>();
  const depths = analysis.depths;
  let changed = true;
  while (changed) {
    changed = false;
    for (let index = scope.start; index <= scope.end; index += 1) {
      const declaration = /\blet\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*(?::[^=;]+)?=/.exec(
        analysis.codeLines[index]
      );
      if (!declaration || aliases.has(declaration[1])) {
        continue;
      }
      const end = statementEnd(analysis.codeLines, index, scope.end, depths);
      const rawText = analysis.rawLines.slice(index, end + 1).join("\n");
      const codeText = analysis.codeLines.slice(index, end + 1).join("\n");
      const initializer = bindingInitializer(rawText, codeText);
      if (
        directDiagnosticEnvCheck(initializer.raw, initializer.code, analysis.constants) ||
        hasHelperCall(initializer.code, envHelpers) ||
        [...aliases].some((alias) => new RegExp(`\\b${alias}\\b`).test(initializer.code))
      ) {
        aliases.add(declaration[1]);
        changed = true;
      }
      index = end;
    }
  }
  return aliases;
}

export function diagnosticGateLine(
  rawText: string,
  codeText: string,
  constants: Set<string>,
  envHelpers: Set<string>,
  localAliases: Set<string>
): boolean {
  if (!/\bif\b/.test(codeText)) {
    return false;
  }
  if (directDiagnosticEnvCheck(rawText, codeText, constants)) {
    return true;
  }
  if (hasHelperCall(codeText, envHelpers)) {
    return true;
  }
  const aliasConditionText = /\bif\s+let\b/.test(codeText)
    ? bindingInitializer(codeText, codeText).code
    : codeText;
  return [...localAliases].some((name) =>
    new RegExp(`\\b${name}\\b`).test(aliasConditionText)
  );
}

export function gateHeaderEnd(codeLines: readonly string[], start: number, maximumEnd: number): number {
  const startIf = codeLines[start].search(/\bif\b/);
  if (startIf !== -1 && codeLines[start].slice(startIf).includes("{")) {
    return start;
  }
  for (let index = start; index <= maximumEnd; index += 1) {
    if (braceDelta(codeLines[index]) > 0 || codeLines[index].trimEnd().endsWith("{")) {
      return index;
    }
  }
  return start;
}

export function hasStructuredCollection(
  rawLines: readonly string[],
  codeLines: readonly string[],
  depths: readonly number[],
  start: number,
  gateLine: number,
  gateEnd: number,
  structuredHelpers: Set<string>,
  stderrHelpers: Set<string>,
  currentScopeName: string
): boolean {
  if (gateLine <= start) {
    return false;
  }
  const mirrorRawText = rawLines.slice(gateLine, gateEnd + 1).join("\n");
  const mirrorCodeText = codeLines.slice(gateLine, gateEnd + 1).join("\n");
  const mirrorHeaderEnd = gateHeaderEnd(codeLines, gateLine, gateEnd);
  const mirrorHeaderCodeText = codeLines.slice(gateLine, mirrorHeaderEnd + 1).join("\n");
  const aliases = iteratorAliases(codeLines, depths, start, gateLine, depths[gateLine]);
  const mirrorTokens = expandSemanticTokensThroughBindings(
    diagnosticSideEffectTokens(mirrorRawText, mirrorCodeText, stderrHelpers),
    rawLines,
    codeLines,
    depths,
    start,
    gateLine,
    depths[gateLine]
  );
  let endExclusive = gateLine;
  while (endExclusive > start) {
    const range = previousStatementRange(codeLines, endExclusive, depths[gateLine], depths, start);
    if (range === null || range[0] < start) {
      return false;
    }
    const rawText = rawLines.slice(range[0], range[1] + 1).join("\n");
    const codeText = codeLines.slice(range[0], range[1] + 1).join("\n");
    const structuredProducer = hasStructuredProducer(
      codeLines,
      range[0],
      range[1],
      structuredHelpers,
      currentScopeName
    );
    if (
      isAssociationBarrier(codeText) &&
      (!structuredProducer ||
        !barrierControlAllowsMirror(codeText, mirrorHeaderCodeText, mirrorCodeText, aliases))
    ) {
      return false;
    }
    if (structuredProducer) {
      const collectionTokens = expandSemanticTokensThroughBindings(
        producerTokens(rawText, codeText, structuredHelpers, currentScopeName),
        rawLines,
        codeLines,
        depths,
        start,
        range[0],
        depths[gateLine]
      );
      if (hasSemanticAssociation(collectionTokens, mirrorTokens)) {
        return true;
      }
    }
    endExclusive = range[0];
  }
  return false;
}

export function barrierControlAllowsMirror(
  barrierText: string,
  gateHeaderText: string,
  gateBlockText: string,
  aliases: ReadonlyMap<string, string>
): boolean {
  const firstLine = barrierText
    .split("\n")
    .map((line) => line.trim())
    .find((line) => line.length > 0);
  if (firstLine?.startsWith("if ")) {
    return gateConditionImpliesCollector(barrierText, gateHeaderText);
  }
  if (firstLine?.startsWith("for ")) {
    const collectorIterators = loopIteratorExpressions(barrierText);
    const mirrorIterators = loopIteratorExpressions(gateBlockText);
    return (
      collectorIterators.length > 0 &&
      collectorIterators.some((collectorIterator) =>
        mirrorIterators.some((mirrorIterator) =>
          equivalentIteratorDataFlow(collectorIterator, mirrorIterator, aliases)
        )
      )
    );
  }
  return false;
}

export type NormalizedCondition = {
  hasOr: boolean;
  terms: Map<string, boolean>;
  valid: boolean;
};

export function gateConditionImpliesCollector(
  collectorControlText: string,
  gateHeaderText: string
): boolean {
  const collector = normalizedCondition(collectorControlText);
  const gate = normalizedCondition(gateHeaderText);
  if (!collector.valid || !gate.valid || collector.hasOr || gate.hasOr) {
    return false;
  }
  return [...collector.terms].every(
    ([term, polarity]) => gate.terms.get(term) === polarity
  );
}

export function normalizedCondition(controlText: string): NormalizedCondition {
  const lines = controlText.split("\n");
  const headerEnd = gateHeaderEnd(lines, 0, lines.length - 1);
  const header = lines.slice(0, headerEnd + 1).join("\n");
  const ifIndex = header.search(/\bif\b/);
  const openingBrace = header.lastIndexOf("{");
  if (ifIndex === -1 || openingBrace === -1 || openingBrace <= ifIndex) {
    return { hasOr: false, terms: new Map(), valid: false };
  }
  const expression = header.slice(ifIndex + 2, openingBrace).trim();
  const split = splitTopLevelBooleanExpression(expression);
  const terms = new Map<string, boolean>();
  let valid = split.parts.length > 0;
  for (const part of split.parts) {
    let atom = part.trim();
    let polarity = true;
    while (atom.startsWith("!") && !atom.startsWith("!=")) {
      polarity = !polarity;
      atom = atom.slice(1).trim();
    }
    atom = stripBalancedOuterParentheses(atom).replace(/\s+/g, "");
    if (atom.length === 0 || (terms.has(atom) && terms.get(atom) !== polarity)) {
      valid = false;
      continue;
    }
    terms.set(atom, polarity);
  }
  return { hasOr: split.hasOr, terms, valid };
}

export function splitTopLevelBooleanExpression(expression: string): {
  hasOr: boolean;
  parts: string[];
} {
  const parts: string[] = [];
  let start = 0;
  let parentheses = 0;
  let brackets = 0;
  let braces = 0;
  let hasOr = false;
  for (let index = 0; index < expression.length - 1; index += 1) {
    const character = expression[index];
    if (character === "(") parentheses += 1;
    else if (character === ")") parentheses -= 1;
    else if (character === "[") brackets += 1;
    else if (character === "]") brackets -= 1;
    else if (character === "{") braces += 1;
    else if (character === "}") braces -= 1;
    if (parentheses !== 0 || brackets !== 0 || braces !== 0) {
      continue;
    }
    const operator = expression.slice(index, index + 2);
    if (operator === "&&" || operator === "||") {
      parts.push(expression.slice(start, index));
      hasOr ||= operator === "||";
      start = index + 2;
      index += 1;
    }
  }
  parts.push(expression.slice(start));
  return { hasOr, parts: parts.filter((part) => part.trim().length > 0) };
}

export function stripBalancedOuterParentheses(value: string): string {
  let result = value.trim();
  while (result.startsWith("(") && result.endsWith(")")) {
    let depth = 0;
    let wrapsWholeValue = true;
    for (let index = 0; index < result.length; index += 1) {
      if (result[index] === "(") depth += 1;
      else if (result[index] === ")") depth -= 1;
      if (depth === 0 && index < result.length - 1) {
        wrapsWholeValue = false;
        break;
      }
    }
    if (!wrapsWholeValue) break;
    result = result.slice(1, -1).trim();
  }
  return result;
}

export function loopIteratorExpressions(text: string): string[] {
  const iterators: string[] = [];
  for (const match of text.matchAll(/\bfor\s+[A-Za-z_][A-Za-z0-9_]*\s+in\s+/g)) {
    const expressionStart = (match.index ?? 0) + match[0].length;
    let parentheses = 0;
    let brackets = 0;
    for (let index = expressionStart; index < text.length; index += 1) {
      const character = text[index];
      if (character === "(") parentheses += 1;
      else if (character === ")") parentheses -= 1;
      else if (character === "[") brackets += 1;
      else if (character === "]") brackets -= 1;
      else if (character === "{" && parentheses === 0 && brackets === 0) {
        iterators.push(text.slice(expressionStart, index).replace(/\s+/g, ""));
        break;
      }
    }
  }
  return iterators;
}

export function iteratorAliases(
  codeLines: readonly string[],
  depths: readonly number[],
  start: number,
  end: number,
  scopeDepth: number
): Map<string, string> {
  const aliases = new Map<string, string>();
  for (let index = start; index < end; index += 1) {
    if (depths[index] !== scopeDepth) {
      continue;
    }
    const match = /\blet\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)(?:\s*:\s*[^=;]+)?\s*=\s*&?\s*([A-Za-z_][A-Za-z0-9_]*)\s*;/.exec(
      codeLines[index]
    );
    if (match) {
      aliases.set(match[1], resolvedIteratorAlias(match[2], aliases));
    }
  }
  return aliases;
}

export function resolvedIteratorAlias(name: string, aliases: ReadonlyMap<string, string>): string {
  const seen = new Set<string>();
  let current = name;
  while (aliases.has(current) && !seen.has(current)) {
    seen.add(current);
    current = aliases.get(current)!;
  }
  return current;
}

export function normalizedIteratorExpression(
  expression: string,
  aliases: ReadonlyMap<string, string>
): string {
  return expression
    .replace(/\b[A-Za-z_][A-Za-z0-9_]*\b/g, (name) => resolvedIteratorAlias(name, aliases))
    .replace(/\s+/g, "");
}

export function iteratorDataRoots(
  expression: string,
  aliases: ReadonlyMap<string, string>
): Set<string> {
  const closureBindings = new Set<string>();
  for (const closure of expression.matchAll(/\|([^|]*)\|/g)) {
    for (const binding of closure[1].matchAll(/\b[A-Za-z_][A-Za-z0-9_]*\b/g)) {
      closureBindings.add(binding[0]);
    }
  }
  const ignored = new Set([
    "as",
    "async",
    "await",
    "const",
    "else",
    "false",
    "for",
    "if",
    "in",
    "let",
    "loop",
    "match",
    "move",
    "mut",
    "ref",
    "return",
    "static",
    "true",
    "unsafe",
    "while"
  ]);
  const roots = new Set<string>();
  for (const match of expression.matchAll(/\b[A-Za-z_][A-Za-z0-9_]*\b/g)) {
    const name = match[0];
    const index = match.index ?? 0;
    const before = expression.slice(0, index).trimEnd();
    const after = expression.slice(index + name.length).trimStart();
    if (
      ignored.has(name) ||
      closureBindings.has(name) ||
      before.endsWith(".") ||
      after.startsWith("::") ||
      after.startsWith("(")
    ) {
      continue;
    }
    if (name === "self") {
      const field = /^\.([A-Za-z_][A-Za-z0-9_]*)/.exec(after);
      roots.add(field ? `self.${field[1]}` : name);
    } else {
      roots.add(resolvedIteratorAlias(name, aliases));
    }
  }
  return roots;
}

export function equivalentIteratorDataFlow(
  collectorIterator: string,
  mirrorIterator: string,
  aliases: ReadonlyMap<string, string>
): boolean {
  if (
    normalizedIteratorExpression(collectorIterator, aliases) ===
    normalizedIteratorExpression(mirrorIterator, aliases)
  ) {
    return true;
  }
  const collectorRoots = iteratorDataRoots(collectorIterator, aliases);
  const mirrorRoots = iteratorDataRoots(mirrorIterator, aliases);
  const helperIterator = (expression: string): boolean =>
    /^(?:[A-Za-z_][A-Za-z0-9_]*::)*[A-Za-z_][A-Za-z0-9_]*\s*\(/.test(
      expression.trim()
    );
  return (
    (helperIterator(collectorIterator) || helperIterator(mirrorIterator)) &&
    collectorRoots.size > 0 &&
    collectorRoots.size === mirrorRoots.size &&
    [...collectorRoots].every((root) => mirrorRoots.has(root))
  );
}

export function isAssociationBarrier(text: string): boolean {
  const firstLine = text
    .split("\n")
    .map((line) => line.trim())
    .find((line) => line.length > 0);
  return firstLine !== undefined && /^(?:if|for|match|while|loop)\b/.test(firstLine);
}

export function isBindingBarrier(text: string): boolean {
  const firstLine = text
    .split("\n")
    .map((line) => line.trim())
    .find((line) => line.length > 0);
  return firstLine !== undefined && /^(?:if|match|while|loop)\b/.test(firstLine);
}

export function recordArguments(rawText: string, codeText: string): string[] {
  return [
    ...callArguments(rawText, codeText, "record"),
    ...callArguments(rawText, codeText, "koushi_diagnostics::record"),
    ...callArguments(rawText, codeText, "record_batch"),
    ...callArguments(rawText, codeText, "koushi_diagnostics::record_batch")
  ];
}

export function hasStructuredRecord(codeText: string): boolean {
  return recordArguments(codeText, codeText).length > 0;
}

export function producerTokens(
  rawText: string,
  codeText: string,
  helpers: Set<string>,
  currentScopeName: string
): Set<string> {
  const tokens = new Set<string>();
  for (const argumentsText of recordArguments(rawText, codeText)) {
    addAll(tokens, semanticTokens(argumentsText));
  }
  for (const helper of helpers) {
    if (helper === currentScopeName || !hasNamedCall(codeText, helper)) {
      continue;
    }
    for (const argumentsText of callArguments(rawText, codeText, helper)) {
      addAll(tokens, semanticTokens(argumentsText));
    }
  }
  return expandInlineLoopBindings(tokens, rawText, codeText);
}

export function expandInlineLoopBindings(
  initialTokens: Set<string>,
  rawText: string,
  codeText: string
): Set<string> {
  const tokens = new Set(initialTokens);
  for (const loop of codeText.matchAll(/\bfor\s+([A-Za-z_][A-Za-z0-9_]*)\s+in\s+/g)) {
    if (!tokens.has(loop[1])) {
      continue;
    }
    const expressionStart = (loop.index ?? 0) + loop[0].length;
    const expressionEnd = codeText.indexOf("{", expressionStart);
    if (expressionEnd !== -1) {
      addAll(tokens, semanticTokens(rawText.slice(expressionStart, expressionEnd)));
    }
  }
  return tokens;
}

export function semanticTokens(text: string): Set<string> {
  const tokens = new Set<string>();
  const lexical = lexicalRustView(text);
  for (const token of lexical.code.matchAll(/\b[A-Za-z_][A-Za-z0-9_]*\b/g)) {
    tokens.add(token[0]);
  }

  for (const value of lexical.stringValues) {
    if (/^[a-z][a-z0-9_]*$/.test(value)) {
      tokens.add(value);
    }
    for (const assignment of value.matchAll(
      /(?:^|\s)[A-Za-z_][A-Za-z0-9_]*=\{?([A-Za-z_][A-Za-z0-9_]*)\}?/g
    )) {
      tokens.add(assignment[1]);
    }
    for (const placeholder of value.matchAll(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g)) {
      tokens.add(placeholder[1]);
    }
  }

  return tokens;
}

export function addAll(target: Set<string>, source: Set<string>): void {
  for (const value of source) {
    target.add(value);
  }
}

export function expandSemanticTokensThroughBindings(
  initialTokens: Set<string>,
  rawLines: readonly string[],
  codeLines: readonly string[],
  depths: readonly number[],
  minimumStart: number,
  endExclusive: number,
  parentDepth: number
): Set<string> {
  const tokens = new Set(initialTokens);
  let cursor = endExclusive;
  while (cursor > minimumStart) {
    const range = previousStatementRange(codeLines, cursor, parentDepth, depths, minimumStart);
    if (range === null || range[0] < minimumStart) {
      break;
    }
    const codeText = codeLines.slice(range[0], range[1] + 1).join("\n");
    if (isBindingBarrier(codeText)) {
      break;
    }
    const declaration = /\blet\s+([\s\S]*?)=/.exec(codeText);
    const rawText = rawLines.slice(range[0], range[1] + 1).join("\n");
    for (const token of [...tokens]) {
      for (const method of ["push", "extend"]) {
        for (const argumentsText of callArguments(rawText, codeText, `${token}.${method}`)) {
          addAll(tokens, semanticTokens(argumentsText));
        }
      }
    }
    const boundNames = declaration
      ? [...declaration[1].matchAll(/\b[A-Za-z_][A-Za-z0-9_]*\b/g)].map((match) => match[0])
      : [];
    if (declaration && boundNames.some((name) => tokens.has(name))) {
      const equals = codeText.indexOf("=", declaration.index);
      const semicolon = codeText.lastIndexOf(";");
      if (equals !== -1 && semicolon > equals) {
        addAll(tokens, semanticTokens(rawText.slice(equals + 1, semicolon)));
      }
    }
    cursor = range[0];
  }
  return tokens;
}

export function diagnosticSideEffectTokens(
  rawText: string,
  codeText: string,
  stderrHelpers: Set<string>
): Set<string> {
  const tokens = new Set<string>();
  for (const argumentsText of callArguments(rawText, codeText, "eprintln!")) {
    addAll(tokens, semanticTokens(argumentsText));
  }
  for (const helper of stderrHelpers) {
    if (!hasNamedCall(codeText, helper)) {
      continue;
    }
    for (const argumentsText of callArguments(rawText, codeText, helper)) {
      addAll(tokens, semanticTokens(argumentsText));
    }
  }
  return expandInlineLoopBindings(tokens, rawText, codeText);
}

export function hasSemanticAssociation(collectionTokens: Set<string>, mirrorTokens: Set<string>): boolean {
  return [...collectionTokens].some((token) => mirrorTokens.has(token));
}

export function hasStructuredProducer(
  codeLines: readonly string[],
  start: number,
  end: number,
  helpers: Set<string>,
  currentScopeName: string
): boolean {
  if (end < start) {
    return false;
  }
  const text = codeLines.slice(start, end + 1).join("\n");
  if (hasStructuredRecord(text)) {
    return true;
  }
  return hasHelperCall(text, helpers, currentScopeName);
}

export function hasDiagnosticSideEffect(
  codeLines: readonly string[],
  start: number,
  end: number,
  stderr: Set<string>,
  structuredHelpers: Set<string>,
  currentScopeName: string
): boolean {
  const text = codeLines.slice(start, end + 1).join("\n");
  if (/\beprintln!\s*\(/.test(text)) {
    return true;
  }
  if (hasHelperCall(text, stderr, currentScopeName)) {
    return true;
  }
  return hasStructuredProducer(codeLines, start, end, structuredHelpers, currentScopeName);
}

export function scanDiagnosticSources(sources: readonly DiagnosticSource[]): DiagnosticGateFinding[] {
  const findings: DiagnosticGateFinding[] = [];
  const analyses = sources.map(({ relativePath, source }): DiagnosticSourceAnalysis => {
    const rawLines = productionRustLines(source);
    const codeLines = lexicalRustView(rawLines.join("\n")).code.split("\n");
    return {
      relativePath,
      rawLines,
      codeLines,
      depths: braceDepths(codeLines),
      scopes: sourceScopes(codeLines),
      constants: envConstants(rawLines, codeLines),
      moduleQualifiers: moduleQualifiers(relativePath)
    };
  });
  const envResolution = resolveHelpers(
    analyses,
    (analysis, scope) =>
      scopeReturnsBool(analysis, scope) &&
      directDiagnosticEnvCheck(
        analysis.rawLines.slice(scope.start, scope.end + 1).join("\n"),
        analysis.codeLines.slice(scope.start, scope.end + 1).join("\n"),
        analysis.constants
      ),
    true,
    scopeReturnsBool
  );
  const structuredResolution = resolveHelpers(
    analyses,
    (analysis, scope) =>
      hasStructuredRecord(analysis.codeLines.slice(scope.start, scope.end + 1).join("\n")),
    true
  );
  const stderrResolution = resolveHelpers(
    analyses,
    (analysis, scope) =>
      /\beprintln!\s*\(/.test(analysis.codeLines.slice(scope.start, scope.end + 1).join("\n")),
    true
  );

  for (const analysis of analyses) {
    const envHelpers = visibleHelpers(envResolution, analysis.relativePath);
    const structuredHelpers = visibleHelpers(structuredResolution, analysis.relativePath);
    const stderr = visibleHelpers(stderrResolution, analysis.relativePath);
    for (const scope of analysis.scopes) {
      const localAliases = localEnvironmentAliases(analysis, scope, envHelpers);
      for (let lineIndex = scope.start; lineIndex <= scope.end; lineIndex += 1) {
        if (!/\bif\b/.test(analysis.codeLines[lineIndex])) {
          continue;
        }
        const headerEnd = gateHeaderEnd(analysis.codeLines, lineIndex, scope.end);
        const headerRawText = analysis.rawLines.slice(lineIndex, headerEnd + 1).join("\n");
        const headerCodeText = analysis.codeLines.slice(lineIndex, headerEnd + 1).join("\n");
        if (
          !diagnosticGateLine(
            headerRawText,
            headerCodeText,
            analysis.constants,
            envHelpers,
            localAliases
          )
        ) {
          continue;
        }
        const blockGateEnd = Math.min(blockEnd(analysis.codeLines, lineIndex), scope.end);
        const gateBlockCode = analysis.codeLines.slice(lineIndex, blockGateEnd + 1).join("\n");
        const negativeEarlyExit =
          /\bif\s*!/.test(headerCodeText) && /\breturn\b/.test(gateBlockCode);
        const diagnosticEnd = negativeEarlyExit ? scope.end : blockGateEnd;
        if (
          !hasDiagnosticSideEffect(
            analysis.codeLines,
            lineIndex,
            diagnosticEnd,
            stderr,
            structuredHelpers,
            scope.name
          )
        ) {
          continue;
        }
        const gatedStructuredProducer = hasStructuredProducer(
          analysis.codeLines,
          lineIndex,
          diagnosticEnd,
          structuredHelpers,
          scope.name
        );
        const hasCollection = hasStructuredCollection(
          analysis.rawLines,
          analysis.codeLines,
          analysis.depths,
          scope.start,
          lineIndex,
          diagnosticEnd,
          structuredHelpers,
          stderr,
          scope.name
        );
        if (gatedStructuredProducer || !hasCollection) {
          findings.push({
            relativePath: analysis.relativePath,
            line: lineIndex + 1,
            location: `${analysis.relativePath}:${lineIndex + 1}`,
            reason: GATED_DIAGNOSTIC_REASON
          });
        }
      }
    }
  }
  return findings;
}

export function runtimeDiagnosticStderrFindings(
  sources: DiagnosticSource[]
): DiagnosticGateFinding[] {
  return sources.flatMap(({ relativePath, source }) => {
    const rawLines = productionRustLines(source);
    const lexical = lexicalRustView(rawLines.join("\n"));
    const code = lexical.code;
    const codeLines = code.split("\n");
    const lineStarts = [0];
    for (const line of codeLines.slice(0, -1)) {
      lineStarts.push(lineStarts.at(-1)! + line.length + 1);
    }
    const removedLiteralLines = new Set(
      lexical.stringSpans
        .filter(({ value }) => REMOVED_DIAGNOSTIC_ENV_LITERALS.includes(value))
        .map(({ start }) => lineIndexAtOffset(lineStarts, start))
    );
    return rawLines.flatMap((_, index) => {
      const codeLine = codeLines[index];
      const reason = /\beprintln!\s*\(/.test(codeLine)
        ? "runtime diagnostic writes to stderr"
        : removedDiagnosticEnvironmentLiteralInSyntax(
              code,
              lineStarts,
              index,
              removedLiteralLines.has(index)
            )
          ? "runtime diagnostic environment gate remains"
          : null;
      return reason
        ? [{ relativePath, line: index + 1, location: `${relativePath}:${index + 1}`, reason }]
        : [];
    });
  });
}

export function removedDiagnosticEnvironmentLiteralInSyntax(
  code: string,
  lineStarts: readonly number[],
  lineIndex: number,
  includesRemovedLiteral: boolean
): boolean {
  const codeLineStart = lineStarts[lineIndex];
  const statementStart =
    Math.max(
      code.lastIndexOf(";", codeLineStart),
      code.lastIndexOf("{", codeLineStart),
      code.lastIndexOf("}", codeLineStart)
    ) + 1;
  const statementEndOffset = code.slice(codeLineStart).search(/[;{}]/);
  const statementEnd =
    statementEndOffset === -1 ? code.length : codeLineStart + statementEndOffset + 1;
  const syntax = code.slice(statementStart, statementEnd);
  return (
    includesRemovedLiteral &&
    (/\bconst\s+[A-Z][A-Z0-9_]*\s*:\s*&str\s*=/.test(syntax) ||
      /\bstd::env::(?:var_os|var)\s*\(/.test(syntax))
  );
}

export function lineIndexAtOffset(lineStarts: readonly number[], offset: number): number {
  let lineIndex = 0;
  while (lineIndex + 1 < lineStarts.length && lineStarts[lineIndex + 1] <= offset) {
    lineIndex += 1;
  }
  return lineIndex;
}
