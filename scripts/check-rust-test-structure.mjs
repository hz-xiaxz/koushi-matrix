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

function readRustSource(relativePath) {
  return fs.readFileSync(path.join(repositoryRoot, relativePath), "utf8");
}

function readTauriSource(relativePath) {
  return readRustSource(`apps/desktop/src-tauri/src/${relativePath}`);
}

function productionOnly(source, fileName) {
  const modules = moduleInventory(source, fileName).inline.slice().sort((left, right) => left.start - right.start);
  let cursor = 0;
  let result = "";
  for (const module of modules) {
    result += source.slice(cursor, module.start);
    cursor = module.end;
  }
  return result + source.slice(cursor);
}

function tauriCommandsSource() {
  return [
    "commands/account.rs",
    "commands/activity.rs",
    "commands/diagnostics.rs",
    "commands/directory.rs",
    "commands/e2ee.rs",
    "commands/live_signals.rs",
    "commands/local_encryption.rs",
    "commands/mod.rs",
    "commands/native_attention.rs",
    "commands/navigation.rs",
    "commands/profile.rs",
    "commands/room.rs",
    "commands/search.rs",
    "commands/session.rs",
    "commands/settings.rs",
    "commands/timeline.rs",
    "commands/views.rs"
  ].map((relativePath) => productionOnly(readTauriSource(relativePath), relativePath)).join("\n");
}

function sourceSection(source, startMarker, endMarker) {
  const start = source.indexOf(startMarker);
  if (start < 0) return null;
  const rest = source.slice(start + startMarker.length);
  const end = endMarker ? rest.indexOf(endMarker) : -1;
  return end < 0 ? rest : rest.slice(0, end);
}

function orderedMarkers(rule, source, markers) {
  const positions = markers.map((marker) => source.indexOf(marker));
  const failures = [];
  if (positions.some((position) => position < 0)) {
    failures.push(sourceContractFailure(rule, "required source marker is missing"));
  } else if (positions.some((position, index) => index > 0 && positions[index - 1] >= position)) {
    failures.push(sourceContractFailure(rule, "required source markers are out of order"));
  }
  return failures;
}

function tauriCommandNames(source) {
  const tokens = lexRust(source);
  const names = [];
  for (let index = 0; index + 9 < tokens.length; index += 1) {
    const values = tokens.slice(index, index + 10).map((token) => token.value);
    if (values.slice(0, 7).join("") !== "#[tauri::command]" || values[7] !== "pub" || values[8] !== "async" || values[9] !== "fn") continue;
    const name = tokens[index + 10]?.value;
    if (tokens[index + 10]?.kind === "identifier") names.push(name);
  }
  return names;
}

export function checkDesktopTauriCommandRegistrationContract() {
  const rule = "desktop.commands.tauri_command_registration";
  const source = tauriCommandsSource();
  const libSource = productionOnly(readTauriSource("lib.rs"), "apps/desktop/src-tauri/src/lib.rs");
  const handlerStart = libSource.indexOf("tauri::generate_handler![");
  const handlerEnd = handlerStart < 0 ? -1 : libSource.indexOf("]", handlerStart);
  const handler = handlerStart >= 0 && handlerEnd >= 0 ? libSource.slice(handlerStart, handlerEnd) : null;
  const names = tauriCommandNames(source);
  const failures = [];
  if (!handler) failures.push(sourceContractFailure(rule, "generate_handler! is missing or unclosed"));
  if (names.length === 0) failures.push(sourceContractFailure(rule, "no Tauri commands found"));
  for (const name of names) {
    if (!handler?.split("\n").some((line) => line.includes("commands::") && line.includes(`::${name}`))) {
      failures.push(sourceContractFailure(rule, `Tauri command registration is missing for ${name}`));
    }
  }
  return failures;
}

export function checkDesktopSubmitCoreCommandContract() {
  const rule = "desktop.commands.submit_core_command_contract";
  const source = readTauriSource("commands/mod.rs");
  const body = rustItemBody(source, "pub(crate) async fn submit_core_command");
  const failures = [];
  for (const marker of ["const CORE_COMMAND_SUBMIT_TIMEOUT", "command_handle", "tokio::time::timeout(CORE_COMMAND_SUBMIT_TIMEOUT"]) {
    if (!source.includes(marker) && !body?.includes(marker)) failures.push(sourceContractFailure(rule, `missing ${marker}`));
  }
  if (body?.includes(".lock()\n        .await\n        .command(command)\n        .await") || body?.includes(".lock().await.command(command).await")) {
    failures.push(sourceContractFailure(rule, "submit_core_command holds the connection mutex while awaiting send"));
  }
  return failures;
}

export function checkDesktopEventWaitLagContract() {
  const rule = "desktop.commands.event_wait_lag_contract";
  const source = tauriCommandsSource();
  const waiters = [
    "async fn wait_for_invite_workflow_snapshot_from",
    "async fn wait_for_logged_in_authenticated",
    "async fn wait_for_auth_changed",
    "async fn wait_for_focused_context_closed",
    "async fn wait_for_focused_context",
    "async fn wait_for_main_timeline_anchor",
    "async fn wait_for_search_started",
    "async fn wait_for_search_closed",
    "async fn wait_for_upload_staging_snapshot",
    "async fn wait_for_room_created",
    "async fn wait_for_space_created",
    "async fn wait_for_room_operation",
    "async fn wait_for_room_joined",
    "async fn wait_for_invite_batch_completed",
    "async fn wait_for_oidc_authorization"
  ];
  const failures = [];
  for (const start of waiters) {
    const body = rustItemBody(source, start);
    if (!body) failures.push(sourceContractFailure(rule, `missing wait path ${start}`));
    else if (body.includes("event stream lagged")) failures.push(sourceContractFailure(rule, `lag is treated as terminal in ${start}`));
  }
  return failures;
}

export function checkDesktopFailureWaiterContract() {
  const rule = "desktop.commands.failure_waiter_contract";
  const source = tauriCommandsSource();
  const waiters = [
    "async fn wait_for_logged_in_authenticated",
    "async fn wait_for_focused_context_closed",
    "async fn wait_for_focused_context",
    "async fn wait_for_main_timeline_anchor",
    "async fn wait_for_search_started",
    "async fn wait_for_search_closed",
    "async fn wait_for_upload_staging_snapshot",
    "async fn wait_for_room_created",
    "async fn wait_for_space_created",
    "async fn wait_for_room_operation",
    "async fn wait_for_room_joined",
    "async fn wait_for_invite_batch_completed",
    "async fn wait_for_oidc_authorization",
    "pub async fn list_saved_sessions"
  ];
  const failures = [];
  for (const start of waiters) {
    const body = rustItemBody(source, start);
    if (!body) failures.push(sourceContractFailure(rule, `missing failure wait path ${start}`));
    else if (!body.includes("invoke_error_from_core_failure")) failures.push(sourceContractFailure(rule, `failure kind is not preserved in ${start}`));
  }
  return failures;
}

export function checkDesktopActivityNavigationContract() {
  const rule = "desktop.activity.navigation_contract";
  const body = rustItemBody(readTauriSource("commands/navigation.rs"), "async fn open_anchored_timeline");
  const failures = [];
  for (const marker of ["CloseFocusedContext", "wait_for_focused_context_closed", "select_room_and_wait", "OpenAnchoredTimeline", "wait_for_main_timeline_anchor"]) {
    if (!body?.includes(marker)) failures.push(sourceContractFailure(rule, `missing ${marker}`));
  }
  for (const marker of ["build_subscribe_timeline_command", "EnterAnchoredTimeline", "wait_for_focused_timeline_event", "build_update_navigation_scroll_anchor_command"]) {
    if (body?.includes(marker)) failures.push(sourceContractFailure(rule, `forbidden ${marker}`));
  }
  failures.push(...orderedMarkers(rule, body ?? "", ["CloseFocusedContext", "wait_for_focused_context_closed", "select_room_and_wait", "OpenAnchoredTimeline", "wait_for_main_timeline_anchor"]));
  return failures;
}

export function checkDesktopActivityCommandContract() {
  const rule = "desktop.activity.command_contract";
  const source = tauriCommandsSource();
  const libSource = productionOnly(readTauriSource("lib.rs"), "apps/desktop/src-tauri/src/lib.rs");
  const failures = [];
  for (const [command, builder, route] of [
    ["pub async fn open_activity", "build_open_activity_command", "commands::activity::open_activity"],
    ["pub async fn close_activity", "build_close_activity_command", "commands::activity::close_activity"],
    ["pub async fn set_activity_tab", "build_set_activity_tab_command", "commands::activity::set_activity_tab"],
    ["pub async fn paginate_activity", "build_paginate_activity_command", "commands::activity::paginate_activity"],
    ["pub async fn mark_activity_read", "build_mark_activity_read_command", "commands::activity::mark_activity_read"],
    ["pub async fn retry_activity_resolution", "build_retry_activity_resolution_command", "commands::activity::retry_activity_resolution"],
    ["pub async fn open_files_view", "build_open_files_view_command", "commands::views::open_files_view"],
    ["pub async fn close_files_view", "build_close_files_view_command", "commands::views::close_files_view"]
  ]) {
    if (!source.includes(command) || !source.includes(builder) || !libSource.includes(route)) failures.push(sourceContractFailure(rule, `missing ${command}, ${builder}, or ${route}`));
  }
  return failures;
}

export function checkDesktopLoginWaitContract() {
  const rule = "desktop.session.login_wait_contract";
  const source = readTauriSource("commands/session.rs");
  const helper = rustItemBody(source, "async fn submit_login_and_wait_for_authenticated");
  const waiter = rustItemBody(source, "async fn wait_for_logged_in_authenticated");
  const failures = [];
  if (!helper?.includes("wait_for_logged_in_authenticated")) failures.push(sourceContractFailure(rule, "login helper does not await authenticated state"));
  if (helper?.includes("build_start_sync_command")) failures.push(sourceContractFailure(rule, "login helper starts sync in the Tauri adapter"));
  if (!helper?.includes("LOGIN_EVENT_TIMEOUT")) failures.push(sourceContractFailure(rule, "login helper lacks its timeout"));
  for (const marker of ["AccountEvent::LoggedIn", "OperationFailed", "timeout_at"]) {
    if (!waiter?.includes(marker)) failures.push(sourceContractFailure(rule, `login waiter lacks ${marker}`));
  }
  return failures;
}

export function checkDesktopE2eeCommandContract() {
  const rule = "desktop.e2ee.command_contract";
  const source = tauriCommandsSource();
  const libSource = productionOnly(readTauriSource("lib.rs"), "apps/desktop/src-tauri/src/lib.rs");
  const failures = [];
  for (const [command, builder, route] of [
    ["pub async fn bootstrap_cross_signing", "build_bootstrap_cross_signing_command", "commands::e2ee::bootstrap_cross_signing"],
    ["pub async fn enable_key_backup", "build_enable_key_backup_command", "commands::e2ee::enable_key_backup"],
    ["pub async fn export_room_keys", "build_export_room_keys_command", "commands::e2ee::export_room_keys"],
    ["pub async fn import_room_keys", "build_import_room_keys_command", "commands::e2ee::import_room_keys"],
    ["pub async fn bootstrap_secure_backup", "build_bootstrap_secure_backup_command", "commands::e2ee::bootstrap_secure_backup"],
    ["pub async fn reenable_secure_backup", "build_bootstrap_secure_backup_command", "commands::e2ee::reenable_secure_backup"],
    ["pub async fn change_secure_backup_passphrase", "build_change_secure_backup_passphrase_command", "commands::e2ee::change_secure_backup_passphrase"],
    ["pub async fn accept_verification", "build_accept_verification_command", "commands::e2ee::accept_verification"],
    ["pub async fn confirm_sas_verification", "build_confirm_sas_verification_command", "commands::e2ee::confirm_sas_verification"],
    ["pub async fn cancel_verification", "build_cancel_verification_command", "commands::e2ee::cancel_verification"],
    ["pub async fn reset_identity", "build_reset_identity_command", "commands::e2ee::reset_identity"],
    ["pub async fn cancel_identity_reset", "build_cancel_identity_reset_command", "commands::e2ee::cancel_identity_reset"],
    ["pub async fn submit_identity_reset_password", "build_submit_identity_reset_password_command", "commands::e2ee::submit_identity_reset_password"],
    ["pub async fn submit_identity_reset_oauth", "build_submit_identity_reset_oauth_command", "commands::e2ee::submit_identity_reset_oauth"]
  ]) {
    if (!source.includes(command) || !source.includes(builder) || !libSource.includes(route)) failures.push(sourceContractFailure(rule, `missing E2EE command contract for ${command}`));
  }
  return failures;
}

export function checkDesktopLocalEncryptionCommandContract() {
  const rule = "desktop.local_encryption.command_contract";
  const source = tauriCommandsSource();
  const libSource = productionOnly(readTauriSource("lib.rs"), "apps/desktop/src-tauri/src/lib.rs");
  const failures = [];
  for (const [command, builder, route, registration] of [
    ["pub async fn probe_local_encryption_health", "build_probe_local_encryption_health_command", "AccountCommand::ProbeLocalEncryptionHealth", "commands::local_encryption::probe_local_encryption_health"],
    ["pub async fn reset_local_data", "build_reset_local_data_command", "AccountCommand::ResetLocalData", "commands::local_encryption::reset_local_data"]
  ]) {
    if (!source.includes(command) || !source.includes(builder) || !source.includes(route) || !libSource.includes(registration)) failures.push(sourceContractFailure(rule, `missing local-encryption contract for ${command}`));
  }
  if (!rustItemBody(readTauriSource("commands/local_encryption.rs"), "pub async fn reset_local_data")?.includes("wait_for_local_data_reset")) {
    failures.push(sourceContractFailure(rule, "reset_local_data does not await signed-out projection"));
  }
  return failures;
}

export function checkDesktopProfileCommandContract() {
  const rule = "desktop.profile.command_contract";
  const source = tauriCommandsSource();
  const libSource = productionOnly(readTauriSource("lib.rs"), "apps/desktop/src-tauri/src/lib.rs");
  const failures = [];
  for (const [command, builder, registration] of [["pub async fn set_display_name", "build_set_display_name_command", "commands::profile::set_display_name"], ["pub async fn set_local_user_alias", "build_set_local_user_alias_command", "commands::profile::set_local_user_alias"], ["pub async fn set_avatar", "build_set_avatar_command", "commands::profile::set_avatar"]]) {
    if (!source.includes(command) || !source.includes(builder) || !libSource.includes(registration)) failures.push(sourceContractFailure(rule, `missing profile contract for ${command}`));
  }
  return failures;
}

export function checkDesktopDirectoryStartDmContract() {
  const rule = "desktop.directory.start_dm_contract";
  const body = rustItemBody(readTauriSource("commands/room.rs"), "pub async fn start_direct_message");
  return orderedMarkers(rule, body ?? "", ["wait_for_direct_message_started", "wait_for_room_in_state", "select_room_and_wait"]);
}

export function checkDesktopDirectoryJoinRoomContract() {
  const rule = "desktop.directory.join_room_selection_contract";
  const body = rustItemBody(readTauriSource("commands/directory.rs"), "pub async fn join_directory_room");
  const failures = [];
  for (const marker of ["wait_for_room_joined", "select_room_and_wait", "joined_room_id", "SELECT_ROOM_EVENT_TIMEOUT"]) {
    if (!body?.includes(marker)) failures.push(sourceContractFailure(rule, `join_directory_room lacks ${marker}`));
  }
  failures.push(...orderedMarkers(rule, body ?? "", ["wait_for_room_joined", "select_room_and_wait"]));
  return failures;
}

export function checkDesktopRoomOperationContract() {
  const rule = "desktop.room.operation_wait_contract";
  const source = readTauriSource("commands/room.rs");
  const failures = [];
  for (const [command, event] of [["pub async fn load_room_settings", "RoomSettingsLoaded"], ["pub async fn update_room_setting", "RoomSettingUpdated"], ["pub async fn moderate_room_member", "RoomMemberModerated"], ["pub async fn update_room_member_role", "RoomMemberRoleUpdated"]]) {
    const body = rustItemBody(source, command);
    for (const marker of ["wait_for_room_operation", event, "update_qa_window_title_from_state", "current_snapshot"]) {
      if (!body?.includes(marker)) failures.push(sourceContractFailure(rule, `${command} lacks ${marker}`));
    }
  }
  return failures;
}

export function checkDesktopSpaceOperationContract() {
  const rule = "desktop.room.space_operation_contract";
  const source = readTauriSource("commands/room.rs");
  const libSource = productionOnly(readTauriSource("lib.rs"), "apps/desktop/src-tauri/src/lib.rs");
  const failures = [];
  for (const [command, matcher] of [["pub async fn load_space_members", "space_members_loaded_event_matches"], ["pub async fn invite_user_to_space", "space_member_invite_settled_event_matches"], ["pub async fn cancel_space_invite", "space_member_invite_cancellation_settled_event_matches"], ["pub async fn update_space_member_role", "wait_for_space_member_role_update"]]) {
    const body = rustItemBody(source, command);
    const waiter = command === "pub async fn update_space_member_role" ? matcher : "wait_for_room_operation";
    for (const marker of [waiter, matcher, "current_snapshot"]) {
      if (!body?.includes(marker)) failures.push(sourceContractFailure(rule, `${command} lacks ${marker}`));
    }
  }
  for (const registration of ["commands::room::cancel_space_invite", "commands::room::update_space_member_role"]) {
    if (!libSource.includes(registration)) failures.push(sourceContractFailure(rule, `space operation registration is missing ${registration}`));
  }
  return failures;
}

export function checkDesktopSearchCommandContract() {
  const rule = "desktop.search.command_contract";
  const source = readTauriSource("commands/search.rs");
  const resolver = rustItemBody(source, "fn resolve_search_scope_from_active_room");
  const command = rustItemBody(source, "pub async fn submit_search");
  const helper = rustItemBody(source, "pub(crate) async fn submit_search_production_path");
  const failures = [];
  for (const marker of ["SearchScope::CurrentSpace", "SearchScope::CurrentRoom"]) if (!resolver?.includes(marker)) failures.push(sourceContractFailure(rule, `search scope resolver lacks ${marker}`));
  if (resolver?.includes("unwrap_or(SearchScope::AllRooms)")) failures.push(sourceContractFailure(rule, "search scope resolver collapses to allRooms"));
  for (const marker of ["submit_search_production_path", "current_snapshot"]) if (!command?.includes(marker)) failures.push(sourceContractFailure(rule, `submit_search lacks ${marker}`));
  failures.push(...orderedMarkers(rule, helper ?? "", ["let mut event_conn = state.runtime.attach()", "let request_id = next_request_id(state).await", "io.submit", "io.wait"]));
  if (helper?.includes("let request_id = event_conn.next_request_id()")) failures.push(sourceContractFailure(rule, "search path allocates its request id from the transient event connection"));
  return failures;
}

export function checkDesktopSettingsCommandContract() {
  const rule = "desktop.settings.command_contract";
  const source = tauriCommandsSource();
  const libSource = productionOnly(readTauriSource("lib.rs"), "apps/desktop/src-tauri/src/lib.rs");
  const failures = [];
  for (const [command, builder, route, registration] of [["pub async fn update_settings", "build_update_settings_command", "AppCommand::UpdateSettings", "commands::settings::update_settings"], ["pub async fn set_room_url_preview_override", "build_set_room_url_preview_override_command", "AppCommand::SetRoomUrlPreviewOverride", "commands::settings::set_room_url_preview_override"], ["pub async fn rebuild_search_index", "build_rebuild_search_index_command", "AppCommand::RebuildSearchIndex", "commands::settings::rebuild_search_index"]]) {
    if (!source.includes(command) || !source.includes(builder) || !source.includes(route) || !libSource.includes(registration)) failures.push(sourceContractFailure(rule, `missing settings contract for ${command}`));
  }
  return failures;
}

export function checkDesktopNavigationContract() {
  const rule = "desktop.navigation.command_contract";
  const source = tauriCommandsSource();
  const failures = [];
  const select = rustItemBody(source, "pub async fn select_room");
  for (const marker of ["state.runtime.attach", "select_room_and_wait", "SELECT_ROOM_EVENT_TIMEOUT"]) if (!select?.includes(marker)) failures.push(sourceContractFailure(rule, `select_room lacks ${marker}`));
  for (const marker of ["build_select_room_command", "wait_for_selected_room", "build_subscribe_timeline_command", "account_key_from_snapshot"]) if (select?.includes(marker)) failures.push(sourceContractFailure(rule, `select_room contains forbidden ${marker}`));
  const trace = readTauriSource("commands/timeline.rs");
  const paginate = rustItemBody(readTauriSource("commands/timeline.rs"), "pub async fn paginate_timeline_backwards");
  const previews = rustItemBody(readTauriSource("commands/timeline.rs"), "pub async fn load_link_previews");
  if (!trace.includes("fn trace_tauri_timeline_command") || !trace.includes("desktop.timeline")) failures.push(sourceContractFailure(rule, "timeline trace helper lacks its private source token"));
  if (select?.includes("trace_tauri_timeline_command(\"submit\", \"select_room\"")) failures.push(sourceContractFailure(rule, "select_room emits duplicate adapter submit telemetry"));
  if (!paginate?.includes("trace_tauri_timeline_command(\"submit\", \"paginate_backwards\"")) failures.push(sourceContractFailure(rule, "backfill submit trace is missing"));
  if (!previews?.includes("trace_tauri_timeline_command(\"submit\", \"load_link_previews\"")) failures.push(sourceContractFailure(rule, "link-preview submit trace is missing"));
  const search = rustItemBody(source, "pub async fn select_search_result");
  const anchored = rustItemBody(source, "async fn open_anchored_timeline");
  if (!search?.includes("open_anchored_timeline")) failures.push(sourceContractFailure(rule, "search-result navigation lacks open_anchored_timeline"));
  for (const marker of ["CloseFocusedContext", "OpenAnchoredTimeline", "select_room_and_wait", "wait_for_main_timeline_anchor", "state.runtime.attach"]) if (!anchored?.includes(marker)) failures.push(sourceContractFailure(rule, `anchored navigation lacks ${marker}`));
  for (const marker of ["EnterAnchoredTimeline", "wait_for_focused_timeline_event", "build_subscribe_timeline_command"]) if (anchored?.includes(marker)) failures.push(sourceContractFailure(rule, `anchored navigation contains forbidden ${marker}`));
  failures.push(...orderedMarkers(rule, anchored ?? "", ["select_room_and_wait", "OpenAnchoredTimeline", "wait_for_main_timeline_anchor"]));
  const close = rustItemBody(source, "pub async fn close_focused_context");
  for (const marker of ["CloseFocusedContext", "update_qa_window_title_from_state", "current_snapshot"]) if (!close?.includes(marker)) failures.push(sourceContractFailure(rule, `close_focused_context lacks ${marker}`));
  failures.push(...orderedMarkers(rule, close ?? "", ["CloseFocusedContext", "wait_for_focused_context_closed", "current_snapshot"]));
  return failures;
}

export function checkDesktopSpaceTraceContract() {
  const rule = "desktop.navigation.space_trace_contract";
  const body = rustItemBody(readTauriSource("commands/navigation.rs"), "pub async fn select_space");
  const failures = orderedMarkers(rule, body ?? "", ["\"desktop.space.transition\", \"submit\"", "build_select_space_command", "\"snapshot\""]);
  for (const marker of ["DiagnosticField::request_id", "DiagnosticField::milliseconds", "DiagnosticField::boolean"]) if (!body?.includes(marker)) failures.push(sourceContractFailure(rule, `space transition trace lacks ${marker}`));
  return failures;
}

export function checkDesktopTimelineGenerationAckContract() {
  const rule = "desktop.timeline.generation_ack_contract";
  const body = rustItemBody(readTauriSource("commands/navigation.rs"), "pub async fn acknowledge_timeline_batch_rendered");
  const failures = [];
  for (const marker of ["key: TimelineKey", "actor_generation: u64", "timeline_generation: TimelineGeneration", "repair_generation: u64", "batch_id: TimelineBatchId", "AppCommand::AcknowledgeTimelineBatchRendered"]) if (!body?.includes(marker)) failures.push(sourceContractFailure(rule, `timeline ACK lacks ${marker}`));
  return failures;
}

export function checkDesktopTimelineCommandContract() {
  const rule = "desktop.timeline.command_contract";
  const source = readTauriSource("commands/timeline.rs");
  const libSource = productionOnly(readTauriSource("lib.rs"), "apps/desktop/src-tauri/src/lib.rs");
  const body = rustItemBody(source, "pub async fn resolve_composer_key_action");
  const failures = [];
  for (const marker of ["koushi_state::resolve_composer_key_action", "settings.values.keyboard.composer_send_shortcut"]) if (!body?.includes(marker)) failures.push(sourceContractFailure(rule, `composer resolver command lacks ${marker}`));
  const threadCommand = rustItemBody(source, "pub async fn paginate_thread_timeline_backwards");
  const threadBuilder = rustItemBody(source, "build_paginate_thread_timeline_backwards_command");
  for (const marker of ["TimelineKind::Thread", "PaginationDirection::Backward", "event_count: TIMELINE_BACKWARDS_PAGE_EVENT_COUNT"]) if (!threadBuilder?.includes(marker)) failures.push(sourceContractFailure(rule, `thread pagination builder lacks ${marker}`));
  if (!threadCommand) failures.push(sourceContractFailure(rule, "thread pagination command is missing"));
  for (const registration of ["commands::timeline::resolve_composer_key_action", "commands::timeline::paginate_thread_timeline_backwards"]) {
    if (!libSource.includes(registration)) failures.push(sourceContractFailure(rule, `timeline command registration is missing ${registration}`));
  }
  return failures;
}

export function checkDesktopTimelineSignalContract() {
  const rule = "desktop.timeline.signal_contract";
  const source = readTauriSource("commands/timeline.rs") + readTauriSource("commands/live_signals.rs");
  const libSource = productionOnly(readTauriSource("lib.rs"), "apps/desktop/src-tauri/src/lib.rs");
  const failures = [];
  for (const [command, kind] of [["pub async fn send_reaction", "send_reaction"], ["pub async fn redact_reaction", "redact_reaction"], ["pub async fn send_read_receipt", "send_read_receipt"], ["pub async fn set_fully_read", "set_fully_read"]]) {
    const body = rustItemBody(source, command);
    for (const marker of [`trace_tauri_timeline_command(\"submit\", \"${kind}\"`, `trace_tauri_timeline_command_elapsed(\n        \"done\",\n        \"${kind}\"`]) if (!body?.includes(marker)) failures.push(sourceContractFailure(rule, `${command} lacks ${kind} trace`));
  }
  for (const registration of ["commands::timeline::send_reaction", "commands::timeline::redact_reaction"]) {
    if (!libSource.includes(registration)) failures.push(sourceContractFailure(rule, `timeline signal registration is missing ${registration}`));
  }
  return failures;
}

export function checkDesktopScheduledSendCommandContract() {
  const rule = "desktop.timeline.scheduled_send_contract";
  const source = readTauriSource("commands/timeline.rs");
  const libSource = productionOnly(readTauriSource("lib.rs"), "apps/desktop/src-tauri/src/lib.rs");
  const failures = [];
  for (const [command, builder, registration] of [["pub async fn schedule_send", "build_schedule_send_command", "commands::timeline::schedule_send"], ["pub async fn cancel_scheduled_send", "build_cancel_scheduled_send_command", "commands::timeline::cancel_scheduled_send"], ["pub async fn reschedule_scheduled_send", "build_reschedule_scheduled_send_command", "commands::timeline::reschedule_scheduled_send"]]) if (!source.includes(command) || !source.includes(builder) || !libSource.includes(registration)) failures.push(sourceContractFailure(rule, `missing scheduled-send contract for ${command}`));
  return failures;
}

export function checkDesktopSendQueueCommandContract() {
  const rule = "desktop.timeline.send_queue_contract";
  const source = readTauriSource("commands/timeline.rs");
  const libSource = productionOnly(readTauriSource("lib.rs"), "apps/desktop/src-tauri/src/lib.rs");
  const failures = [];
  for (const [command, builder, registration] of [["pub async fn retry_send", "build_retry_send_command", "commands::timeline::retry_send"], ["pub async fn cancel_send", "build_cancel_send_command", "commands::timeline::cancel_send"]]) if (!source.includes(command) || !source.includes(builder) || !libSource.includes(registration)) failures.push(sourceContractFailure(rule, `missing send-queue contract for ${command}`));
  return failures;
}

export function checkDesktopForwarderLagRecoveryContract() {
  const rule = "desktop.forwarder.lag_recovery_contract";
  const forwarder = productionOnly(readTauriSource("core_event_forwarder.rs"), "apps/desktop/src-tauri/src/core_event_forwarder.rs");
  const root = productionOnly(readTauriSource("lib.rs"), "apps/desktop/src-tauri/src/lib.rs");
  const lag = sourceSection(forwarder, "Err(lag)", "Ok(event)") ?? sourceSection(forwarder, "Err(lag)");
  const failures = [];
  for (const marker of ["TimelineCommand::ReplaySubscribed", "struct CoreEventForwarderTask"]) if (!forwarder.includes(marker)) failures.push(sourceContractFailure(rule, `forwarder lacks ${marker}`));
  if (!root.includes("forwarder_task: Some")) failures.push(sourceContractFailure(rule, "lib.rs does not retain the forwarder task"));
  if (forwarder.includes("Box::leak")) failures.push(sourceContractFailure(rule, "forwarder counter is leaked"));
  for (const marker of ["event_conn.command_handle()", "event_conn.next_request_id()", "emit_forwarded_webview_events", "submit_timeline_replay_after_forwarder_lag"]) if (!lag?.includes(marker)) failures.push(sourceContractFailure(rule, `lag recovery lacks ${marker}`));
  if (lag?.includes("async_runtime::spawn")) failures.push(sourceContractFailure(rule, "lag replay is detached"));
  failures.push(...orderedMarkers(rule, lag ?? "", ["emit_forwarded_webview_events", "submit_timeline_replay_after_forwarder_lag"]));
  return failures;
}

export function checkDesktopQaControlPipeContract() {
  const rule = "desktop.native.qa_control_pipe_cfg";
  const source = productionOnly(readTauriSource("lib.rs"), "apps/desktop/src-tauri/src/lib.rs");
  const failures = [];
  for (const token of ["const QA_CONTROL_PIPE_ENV", "fn qa_control_pipe_path_from_env()", "spawn_qa_control_pipe_reader"]) {
    const offset = source.indexOf(token);
    const gate = source.lastIndexOf("#[cfg(any(debug_assertions, test))]", offset);
    if (offset < 0 || gate < 0 || source.slice(gate, offset).includes("\n\n")) failures.push(sourceContractFailure(rule, `control-pipe item is not directly debug/test gated`));
  }
  if (source.split("std::env::var(QA_CONTROL_PIPE_ENV)").length - 1 !== 1) failures.push(sourceContractFailure(rule, "control-pipe env is read more than once"));
  return failures;
}

export function checkDesktopNativeWindowLifecycleContract() {
  const rule = "desktop.native.window_lifecycle_contract";
  const source = productionOnly(readTauriSource("lib.rs"), "apps/desktop/src-tauri/src/lib.rs");
  const failures = [];
  const destroyed = sourceSection(source, "if window_event_should_stop_background_tasks(event)", ".invoke_handler");
  for (const marker of ["submit_core_shutdown", "AppCommand::Shutdown { request_id }"]) if (!destroyed?.includes(marker) && !source.includes(marker)) failures.push(sourceContractFailure(rule, `window destruction path lacks ${marker}`));
  const helper = rustItemBody(source, "fn window_event_should_stop_background_tasks");
  if (helper?.includes("CloseRequested")) failures.push(sourceContractFailure(rule, "close request stops background tasks"));
  const close = sourceSection(source, "tauri::WindowEvent::CloseRequested", "if window_event_should_persist");
  for (const marker of ["prevent_close()", ".hide()", "window.is_fullscreen()", "window.set_fullscreen(false)"]) if (!close?.includes(marker)) failures.push(sourceContractFailure(rule, `close handler lacks ${marker}`));
  failures.push(...orderedMarkers(rule, close ?? "", ["window.set_fullscreen(false)", "window.hide()"]));
  return failures;
}

export function checkDesktopNativeReopenContract() {
  const rule = "desktop.native.reopen_contract";
  const source = productionOnly(readTauriSource("lib.rs"), "apps/desktop/src-tauri/src/lib.rs");
  const callback = sourceSection(source, "tauri_plugin_single_instance::init(", ".plugin(tauri_plugin_deep_link::init())");
  const run = sourceSection(source, "pub fn run()", "#[cfg(test)]");
  const failures = [];
  for (const marker of ["ensure_main_window_visible_for_handle", "desktop.lifecycle", "reopen_requested"]) if (!callback?.includes(marker)) failures.push(sourceContractFailure(rule, `single-instance callback lacks ${marker}`));
  for (const marker of [".build(tauri::generate_context!())", "tauri::RunEvent::Reopen", "ensure_main_window_visible_for_handle", "desktop.lifecycle", "reopen_requested"]) if (!run?.includes(marker)) failures.push(sourceContractFailure(rule, `run reopen path lacks ${marker}`));
  return failures;
}

export function checkDesktopViewportAdapterIsolationContract() {
  const rule = "desktop.viewport.native_adapter_isolation";
  const source = productionOnly(readTauriSource("viewport_sync.rs"), "apps/desktop/src-tauri/src/viewport_sync.rs");
  const failures = [];
  if (!source.includes("synchronize_now")) failures.push(sourceContractFailure(rule, "native adapter lacks synchronize_now"));
  for (const marker of ["set_size", "dispatchEvent"]) if (source.includes(marker)) failures.push(sourceContractFailure(rule, `native adapter contains forbidden ${marker}`));
  return failures;
}

function rustItemBody(source, marker) {
  const start = source.indexOf(marker);
  if (start < 0) return null;
  const tokens = lexRust(source);
  const pairs = delimiterPairs(tokens);
  const open = tokens.findIndex((token) => token.start >= start && token.kind === "punctuation" && token.value === "{");
  if (open < 0) return null;
  const close = pairs.openToClose.get(open);
  return close === undefined ? null : source.slice(start, tokens[close].end);
}

function sourceContractFailure(rule, message) {
  return { kind: "source-contract", rule, message };
}

export function checkStateFocusedContextReducerContract() {
  const rule = "state.focused_context_reducer_contract";
  const source = readRustSource("crates/koushi-state/src/reducer/mod.rs") + readRustSource("crates/koushi-state/src/reducer/thread.rs");
  const failures = [];
  for (const fragment of ["OpenFocusedContext", "FocusedContextSubscribed", "CloseFocusedContext", "OpenFocusedTimeline"]) {
    if (!source.includes(fragment)) failures.push(sourceContractFailure(rule, `missing focused-context reducer marker ${fragment}`));
  }
  return failures;
}

export function checkStateHasNoLegacySyncModeVocabulary() {
  const rule = "state.no_legacy_sync_mode_vocabulary";
  const source = [
    "crates/koushi-state/src/state/sync.rs",
    "crates/koushi-state/src/state/mod.rs",
    "crates/koushi-state/src/action.rs",
    "crates/koushi-state/src/effect.rs",
    "crates/koushi-state/src/reducer/sync.rs",
    "crates/koushi-state/src/reducer/mod.rs"
  ].map(readRustSource).join("\n");
  const failures = [];
  for (const fragment of ["SyncMode", "SyncModeFailureKind", "SyncModeChanged", "sync_mode", "LegacySync", "Transitioning"]) {
    if (source.includes(fragment)) failures.push(sourceContractFailure(rule, `forbidden sync vocabulary remains: ${fragment}`));
  }
  return failures;
}

export function checkSdkPasswordSmokeRuntimeSafety() {
  const rule = "sdk.password_smoke_runtime_safety";
  const source = readRustSource("crates/koushi-sdk/src/bin/password-login-smoke.rs");
  const failures = [];
  if (source.includes("fn restore_session_with_store_blocking(")) failures.push(sourceContractFailure(rule, "store-backed restore uses a blocking helper"));
  if (!source.includes("runtime.enter()")) failures.push(sourceContractFailure(rule, "store-backed session drop does not enter its runtime"));
  if (!source.includes("session.take()")) failures.push(sourceContractFailure(rule, "store-backed session drop does not take the session"));
  return failures;
}

export function checkSdkClientStoreConfigContract() {
  const rule = "sdk.client_store_config_contract";
  const source = readRustSource("crates/koushi-sdk/src/client_session.rs");
  const config = rustItemBody(source, "impl MatrixClientStoreConfig");
  const apply = rustItemBody(source, "fn apply_to_builder");
  const failures = [];
  if (!config?.includes("fn apply_to_builder")) failures.push(sourceContractFailure(rule, "MatrixClientStoreConfig must keep apply_to_builder"));
  if (!apply?.includes(".key(Some(self.key.expose_key()))")) failures.push(sourceContractFailure(rule, "apply_to_builder must pass the required store key"));
  if (!apply?.includes(".pool_max_size(DESKTOP_SQLITE_STORE_POOL_MAX_SIZE)")) failures.push(sourceContractFailure(rule, "apply_to_builder must cap the SDK SQLite pool"));
  return failures;
}

export function checkSdkDesktopClientBuilderDefaults() {
  const rule = "sdk.desktop_client_builder_defaults";
  const body = rustItemBody(readRustSource("crates/koushi-sdk/src/client_session.rs"), "fn desktop_client_builder_defaults");
  const failures = [];
  for (const fragment of ["with_threading_support", "ThreadingSupport::Enabled", "with_subscriptions: true", "with_enable_share_history_on_invite(true)", "with_encryption_sync_readiness(true)"]) {
    if (!body?.includes(fragment)) failures.push(sourceContractFailure(rule, `desktop builder default is missing ${fragment}`));
  }
  return failures;
}

export function checkSdkBackupDownloadDefault() {
  const rule = "sdk.backup_download_default";
  const body = rustItemBody(readRustSource("crates/koushi-sdk/src/client_session.rs"), "fn desktop_client_builder_defaults");
  const failures = [];
  for (const fragment of ["with_encryption_settings", "BackupDownloadStrategy::AfterDecryptionFailure"]) {
    if (!body?.includes(fragment)) failures.push(sourceContractFailure(rule, `desktop builder default is missing ${fragment}`));
  }
  return failures;
}

export function checkSdkRecoveryUsesSdkSignaturePublication() {
  const rule = "sdk.recovery.uses_sdk_signature_publication";
  const body = rustItemBody(readRustSource("crates/koushi-sdk/src/e2ee.rs"), "pub async fn recover_e2ee");
  const failures = [];
  for (const fragment of ["prepare_current_device_registration", "force_upload_device_keys", ".recover(request.secret.expose_secret())", "republish_current_device_keys_after_recovery", "post_recovery_device_republish"]) {
    if (body?.includes(fragment)) failures.push(sourceContractFailure(rule, `recovery contains forbidden out-of-band publication ${fragment}`));
  }
  for (const fragment of ["recover_and_fix_backup", "get_own_device", "post_recovery_own_device_inspected", "inspect_current_device_signature_state", "is_cross_signed_by_owner", "record_recovery_verification_event"]) {
    if (!body?.includes(fragment)) failures.push(sourceContractFailure(rule, `recovery is missing ${fragment}`));
  }
  return failures;
}

export function checkSdkRecoverySignatureRoundTripContract() {
  const rule = "sdk.recovery.signature_round_trip_contract";
  const devices = readRustSource("vendor/matrix-rust-sdk/crates/matrix-sdk/src/encryption/identities/devices.rs");
  const secretStore = readRustSource("vendor/matrix-rust-sdk/crates/matrix-sdk/src/encryption/secret_storage/secret_store.rs");
  const failures = [];
  if (!devices.includes("verify_with_diagnostics")) failures.push(sourceContractFailure(rule, "the SDK device target lacks diagnostic verification"));
  for (const fragment of ["standard_signature_round_trip_finished", "preupload_self_signing_signature_valid", "signed_content_matches_refreshed", "self_signing_key_id_matches_refreshed", "preupload_signature_matches_refreshed", "preupload_signature_valid_with_refreshed_key"]) {
    if (!secretStore.includes(fragment)) failures.push(sourceContractFailure(rule, `secret-storage recovery diagnostics are missing ${fragment}`));
  }
  if (secretStore.includes("preupload_signature_value")) failures.push(sourceContractFailure(rule, "secret-storage diagnostics expose a raw signature value"));
  return failures;
}

export function checkSdkRoomReadMarkerContract() {
  const rule = "sdk.room_read_marker_contract";
  const body = rustItemBody(readRustSource("crates/koushi-sdk/src/room_operations.rs"), "pub async fn mark_room_as_read");
  const failures = [];
  for (const fragment of ["send_multiple_receipts", "fully_read_marker", "private_read_receipt"]) {
    if (!body?.includes(fragment)) failures.push(sourceContractFailure(rule, `mark_room_as_read is missing ${fragment}`));
  }
  if (body?.includes("send_single_receipt(ReceiptType::FullyRead")) failures.push(sourceContractFailure(rule, "mark_room_as_read sends a standalone fully-read receipt"));
  return failures;
}

export function checkSdkSpaceInviteCancellationContract() {
  const rule = "sdk.space_invite_cancellation_contract";
  const body = rustItemBody(readRustSource("crates/koushi-sdk/src/room_operations.rs"), "pub async fn cancel_space_invite");
  const failures = [];
  const markers = [
    "members_no_sync(matrix_sdk::RoomMemberships::INVITE)",
    "MatrixSpaceInviteCancellationOutcome::NotInvited",
    ".kick_user(",
    "MatrixSpaceInviteCancellationOutcome::Cancelled"
  ];
  const positions = markers.map((marker) => body?.indexOf(marker) ?? -1);
  if (positions.some((position) => position < 0)) failures.push(sourceContractFailure(rule, "invite cancellation is missing its membership, no-op, kick, or success marker"));
  if (positions[0] >= 0 && positions[1] >= 0 && positions[0] >= positions[1]) failures.push(sourceContractFailure(rule, "invite membership is checked after the no-op outcome"));
  if (positions[1] >= 0 && positions[2] >= 0 && positions[1] >= positions[2]) failures.push(sourceContractFailure(rule, "invite cancellation kicks before the no-op outcome"));
  return failures;
}

export function checkSdkRoomTagMethods() {
  const rule = "sdk.room_tag_methods";
  const source = readRustSource("crates/koushi-sdk/src/room_operations.rs");
  const failures = [];
  for (const fragment of ["set_is_favourite(true", "set_is_favourite(false", "set_is_low_priority(true", "set_is_low_priority(false"]) {
    if (!source.includes(fragment)) failures.push(sourceContractFailure(rule, `room tag operation is missing ${fragment}`));
  }
  return failures;
}

export function checkSdkPinnedEventMethods() {
  const rule = "sdk.pinned_event_methods";
  const source = readRustSource("crates/koushi-sdk/src/room_operations.rs");
  const pin = rustItemBody(source, "pub async fn pin_event");
  const unpin = rustItemBody(source, "pub async fn unpin_event");
  const failures = [];
  if (!pin?.includes(".pin_event(&event_id)")) failures.push(sourceContractFailure(rule, "pin_event does not call the SDK pin method"));
  if (!unpin?.includes(".unpin_event(&event_id)")) failures.push(sourceContractFailure(rule, "unpin_event does not call the SDK unpin method"));
  return failures;
}

export function checkSdkRoomManagementMethods() {
  const rule = "sdk.room_management_methods";
  const source = readRustSource("crates/koushi-sdk/src/room_operations.rs");
  const failures = [];
  for (const fragment of [".set_name(", ".set_room_topic(", ".set_avatar_url(", ".remove_avatar(", ".privacy_settings()", ".update_join_rule(", ".update_room_history_visibility(", ".kick_user(", ".ban_user(", ".unban_user(", ".update_power_levels("]) {
    if (!source.includes(fragment)) failures.push(sourceContractFailure(rule, `room management is missing ${fragment}`));
  }
  return failures;
}

export function checkSdkJoinedRoomListDirectDetection() {
  const rule = "sdk.room_projection.async_direct_detection";
  const body = rustItemBody(readRustSource("crates/koushi-sdk/src/room_projection.rs"), "async fn matrix_room_list_snapshot_from_rooms");
  const failures = [];
  for (const fragment of ["room.is_direct().await", "unwrap_or_else(|_| room.is_dm())"]) {
    if (!body?.includes(fragment)) failures.push(sourceContractFailure(rule, `joined room projection is missing ${fragment}`));
  }
  return failures;
}

export function checkSdkJoinedRoomListAvoidsFullMemberScans() {
  const rule = "sdk.room_projection.no_full_member_scan";
  const body = rustItemBody(readRustSource("crates/koushi-sdk/src/room_projection.rs"), "async fn matrix_room_list_snapshot_from_rooms");
  const failures = [];
  for (const fragment of ["room.joined_members_count()", "matrix_space_member_user_ids_no_sync(&room).await"]) {
    if (!body?.includes(fragment)) failures.push(sourceContractFailure(rule, `room-list projection is missing ${fragment}`));
  }
  for (const fragment of ["collect_active_member_profiles", "room.members(matrix_sdk::RoomMemberships::ACTIVE)", "joined_user_ids"]) {
    if (body?.includes(fragment)) failures.push(sourceContractFailure(rule, `room-list projection contains forbidden full-member path ${fragment}`));
  }
  return failures;
}

export function checkSdkDmResolutionCandidates() {
  const rule = "sdk.room_projection.dm_resolution_candidates";
  const body = rustItemBody(readRustSource("crates/koushi-sdk/src/room_projection.rs"), "async fn matrix_room_list_dm_user_ids");
  const failures = [];
  for (const fragment of ["direct_targets_by_room.get(&room_id)", ".direct_targets()", "room.heroes()", "get_member_no_sync", "dm_user_ids.push(candidate_user_id_string.clone())"]) {
    if (!body?.includes(fragment)) failures.push(sourceContractFailure(rule, `DM resolution is missing ${fragment}`));
  }
  for (const fragment of ["room.members(matrix_sdk::RoomMemberships::ACTIVE)", "room.members_no_sync(matrix_sdk::RoomMemberships::ACTIVE)"]) {
    if (body?.includes(fragment)) failures.push(sourceContractFailure(rule, `DM resolution contains forbidden full-member path ${fragment}`));
  }
  return failures;
}

export function checkSdkSpaceMemberIdsNoSync() {
  const rule = "sdk.room_projection.space_member_ids_no_sync";
  const body = rustItemBody(readRustSource("crates/koushi-sdk/src/room_projection.rs"), "async fn matrix_space_member_user_ids_no_sync");
  const failures = [];
  if (!body?.includes("members_no_sync(matrix_sdk::RoomMemberships::JOIN)")) failures.push(sourceContractFailure(rule, "space membership does not use the joined no-sync view"));
  if (body?.includes("RoomMemberships::ACTIVE")) failures.push(sourceContractFailure(rule, "space membership uses the active membership view"));
  if (body?.includes("room.members(matrix_sdk::RoomMemberships::JOIN)")) failures.push(sourceContractFailure(rule, "space membership fetches the joined member list"));
  return failures;
}

export function checkSdkJoinedOnlySpaceMemberProjection() {
  const rule = "sdk.room_projection.joined_only_membership";
  const body = rustItemBody(readRustSource("crates/koushi-sdk/src/room_projection.rs"), "async fn matrix_space_members_projection");
  const failures = [];
  if (body?.includes("RoomMemberships::ACTIVE")) failures.push(sourceContractFailure(rule, "space member projection uses the active membership view"));
  for (const fragment of ["members_no_sync(matrix_sdk::RoomMemberships::JOIN)", "members_no_sync(matrix_sdk::RoomMemberships::INVITE"]) {
    if (!body?.includes(fragment)) failures.push(sourceContractFailure(rule, `space member projection is missing ${fragment}`));
  }
  return failures;
}

export function checkSdkSpaceLookupFailuresPropagate() {
  const rule = "sdk.room_projection.space_lookup_failures_propagate";
  const body = rustItemBody(readRustSource("crates/koushi-sdk/src/room_projection.rs"), "pub async fn matrix_space_members_projection");
  const failures = [];
  const joined = body?.split("let space_joined_members = match")[1]?.split("let space_invited_members")[0];
  const invited = body?.split("let space_invited_members = match")[1]?.split("let mut space_joined_by_user")[0];
  for (const lookup of [joined, invited]) {
    if (!lookup?.includes("Err(error)")) failures.push(sourceContractFailure(rule, "space lookup does not retain its structured error"));
    if (!lookup?.includes("return Err(MatrixRoomOperationError::from_sdk_error(error))")) failures.push(sourceContractFailure(rule, "space lookup does not abort on error"));
  }
  return failures;
}

export function checkSdkFailedSpaceMemberCountsUnavailable() {
  const rule = "sdk.room_projection.failed_counts_unavailable";
  const body = rustItemBody(readRustSource("crates/koushi-sdk/src/room_projection.rs"), "fn space_members_scope_diagnostic_event");
  const failures = [];
  for (const fragment of ["space_join_lookup_outcome", "space_invite_lookup_outcome", "counts_unavailable", "space_joined_lookup.observed_count()", "space_invited_lookup.observed_count()", "if let Some(count)"]) {
    if (!body?.includes(fragment)) failures.push(sourceContractFailure(rule, `space-member diagnostic is missing ${fragment}`));
  }
  return failures;
}

export function checkSdkRoomMemberSummariesUseFullMembers() {
  const rule = "sdk.room_projection.member_summaries_full_members";
  const body = rustItemBody(readRustSource("crates/koushi-sdk/src/room_projection.rs"), "async fn matrix_room_member_summaries");
  return body?.includes("room.members(matrix_sdk::RoomMemberships::ACTIVE)")
    ? []
    : [sourceContractFailure(rule, "member summaries no longer load the full active member list")];
}

export function checkSdkDirectAccountDataLoaderIsLocalOnly() {
  const rule = "sdk.room_projection.direct_account_data_local_only";
  const body = rustItemBody(readRustSource("crates/koushi-sdk/src/room_projection.rs"), "pub async fn cached_direct_account_data_targets_by_room");
  const failures = [];
  if (!body?.includes("account_data::<DirectEventContent>()")) failures.push(sourceContractFailure(rule, "direct account-data loader lacks its local account-data read"));
  if (body?.includes("fetch_account_data_static")) failures.push(sourceContractFailure(rule, "direct account-data loader fetches account data from the server"));
  return failures;
}

export function checkSdkDirectAccountDataServerFallback() {
  const rule = "sdk.room_projection.direct_account_data_server_fallback";
  const body = rustItemBody(readRustSource("crates/koushi-sdk/src/room_projection.rs"), "async fn matrix_direct_account_data_targets_by_room");
  const failures = [];
  for (const fragment of ["account_data::<DirectEventContent>()", "fetch_account_data_static::<DirectEventContent>()"]) {
    if (!body?.includes(fragment)) failures.push(sourceContractFailure(rule, `direct account-data resolution is missing ${fragment}`));
  }
  return failures;
}

export function checkSdkSlidingSyncInviteProbeContract() {
  const rule = "sdk.sync.sliding_sync_invite_probe_contract";
  const source = readRustSource("crates/koushi-sdk/src/sync.rs");
  const start = source.indexOf("pub async fn probe_sliding_sync_invite_list_support");
  const end = source.indexOf("pub fn sync_once_blocking", start);
  const implementation = start >= 0 && end >= 0 ? source.slice(start, end) : null;
  const helper = implementation?.indexOf("async fn build_sliding_sync_invite_probe_client");
  const body = helper === undefined || helper < 0 ? implementation : implementation?.slice(0, helper);
  const failures = [];
  const ordered = [
    "tokio::time::timeout(SYNC_INVITE_PROBE_TIMEOUT, async {",
    "build_sliding_sync_invite_probe_client(session).await",
    "send_sliding_sync_invite_list_probe(&probe).await"
  ].map((fragment) => body?.indexOf(fragment) ?? -1);
  if (ordered.some((position) => position < 0)) failures.push(sourceContractFailure(rule, "invite probe is missing its timeout, client, or request marker"));
  if (ordered.every((position) => position >= 0) && !(ordered[0] < ordered[1] && ordered[1] < ordered[2])) failures.push(sourceContractFailure(rule, "invite probe timeout does not enclose client setup and request"));
  for (const fragment of [".send(request)", "with_request_config", "SYNC_INVITE_PROBE_TIMEOUT", "disable_retry()"]) {
    if (!implementation?.includes(fragment)) failures.push(sourceContractFailure(rule, `invite probe is missing ${fragment}`));
  }
  for (const fragment of [".sliding_sync(", "RoomListService::"]) {
    if (implementation?.includes(fragment)) failures.push(sourceContractFailure(rule, `invite probe contains forbidden live-sync construction ${fragment}`));
  }
  return failures;
}

const sdkLibrarySourcePaths = [
  "src/auth.rs",
  "src/client_session.rs",
  "src/e2ee.rs",
  "src/lib.rs",
  "src/profile.rs",
  "src/qa_reports.rs",
  "src/room_operations.rs",
  "src/room_projection.rs",
  "src/search.rs",
  "src/sliding_sync_discovery.rs",
  "src/sync.rs",
  "src/timeline.rs"
];

export function checkSdkSessionBackupFence() {
  const rule = "sdk.sessions.no_per_send_backup_fence";
  const sources = sdkLibrarySourcePaths.map((relativePath) => readRustSource(`crates/koushi-sdk/${relativePath}`));
  const falseCount = sources.reduce((count, source) => count + source.split("require_secure_backup_for_encrypted_sends(false)").length - 1, 0);
  const failures = [];
  if (falseCount !== 3) failures.push(sourceContractFailure(rule, `expected three disabled per-send backup fences, found ${falseCount}`));
  if (sources.some((source) => source.includes("require_secure_backup_for_encrypted_sends(true)"))) failures.push(sourceContractFailure(rule, "a session constructor enables the per-send backup fence"));
  return failures;
}

export function checkSdkLibrarySourceManifest() {
  const rule = "sdk.library_source_manifest";
  const paths = sdkLibrarySourcePaths.slice();
  const unique = [...new Set(paths)].sort();
  const failures = [];
  if (unique.length !== paths.length) failures.push(sourceContractFailure(rule, "SDK library source manifest contains duplicate paths"));
  if (JSON.stringify(unique) !== JSON.stringify(paths.slice().sort())) failures.push(sourceContractFailure(rule, "SDK library source manifest is not sorted completely"));
  for (const relativePath of paths) {
    try {
      fs.readFileSync(path.join(repositoryRoot, "crates/koushi-sdk", relativePath), "utf8");
    } catch {
      failures.push(sourceContractFailure(rule, `SDK library source manifest target is missing: ${relativePath}`));
    }
  }
  return failures;
}

export function checkSdkCommittedRoomCheckpointHasNoLegacyApi() {
  const rule = "sdk.timeline.committed_room_checkpoint_no_legacy_api";
  const source = sdkLibrarySourcePaths.map((relativePath) => readRustSource(`crates/koushi-sdk/${relativePath}`)).join("\n");
  const failures = [];
  for (const fragment of ["MatrixCommittedRoomTimelineBackend", "MatrixCommittedRoomTimelineOrigin", "MatrixCommittedRoomUpdatesResponse", "from_committed_observation", "from_legacy_gap_for_testing", "from_legacy_room_absent", "is_room_absent"]) {
    if (source.includes(fragment)) failures.push(sourceContractFailure(rule, `legacy room checkpoint API remains: ${fragment}`));
  }
  return failures;
}

function readAccountSource(relativePath) {
  return readRustSource(`crates/koushi-core/src/account/${relativePath}`);
}

function accountProductionSource(relativePath) {
  const fileName = `crates/koushi-core/src/account/${relativePath}`;
  return productionOnly(readRustSource(fileName), fileName);
}

function accountItemBody(relativePath, marker) {
  return rustItemBody(accountProductionSource(relativePath), marker);
}

function accountSection(relativePath, startMarker, endMarker) {
  return sourceSection(accountProductionSource(relativePath), startMarker, endMarker);
}

export function checkCoreAccountSessionReplacementTeardown() {
  const rule = "core.account.session_replacement_teardown";
  const install = accountItemBody("session_lifecycle.rs", "async fn install_provisional_session");
  const teardown = accountItemBody("runtime_children.rs", "async fn stop_current_session_runtime");
  const failures = [];
  if (!install?.includes("stop_current_session_runtime().await")) failures.push(sourceContractFailure(rule, "provisional session installation lacks runtime teardown"));
  if (!teardown?.includes("stop_active_session_account_management_discovery")) failures.push(sourceContractFailure(rule, "runtime teardown lacks account-management discovery cancellation"));
  return failures;
}

export function checkCoreAccountReliableReducerDelivery() {
  const rule = "core.account.reliable_reducer_delivery";
  const sources = [
    "account_management.rs", "actor.rs", "local_data_cleanup.rs", "profile.rs",
    "recovery_backup.rs", "routing.rs", "runtime_children.rs", "scheduled_send.rs",
    "session_lifecycle.rs", "sliding_sync.rs", "trust_gate.rs", "verification.rs"
  ].map(accountProductionSource);
  const failures = [];
  const sendActions = accountItemBody("actor.rs", "async fn send_actions");
  if (!sendActions?.includes("self.action_tx.send(actions).await")) failures.push(sourceContractFailure(rule, "send_actions does not await reliable action delivery"));
  if (sources.some((source) => source.includes("self.reduce("))) failures.push(sourceContractFailure(rule, "AccountActor command-result actions use the lossy reduce helper"));
  if (sources.some((source) => source.includes("action_tx.try_send(actions)"))) failures.push(sourceContractFailure(rule, "AccountActor actions use drop-on-full try_send"));
  return failures;
}

export function checkCoreAccountLoginHydrationOrder() {
  const rule = "core.account.login_hydration_order";
  const login = accountItemBody("session_lifecycle.rs", "async fn handle_login_password");
  const promotion = accountItemBody("trust_gate.rs", "async fn handle_trust_projection_applied");
  const failures = [];
  const loggedIn = login?.indexOf("AccountEvent::LoggedIn") ?? -1;
  if (loggedIn < 0) failures.push(sourceContractFailure(rule, "login handler does not emit LoggedIn"));
  for (const marker of [
    "own_profile_action_from_session(&session_arc).await",
    "local_user_aliases_action_from_session(&session_arc).await",
    "ignored_user_ids_action_from_session(&session_arc).await"
  ]) {
    const position = login?.indexOf(marker) ?? -1;
    if (position >= 0 && position <= loggedIn) failures.push(sourceContractFailure(rule, `optional hydration precedes LoggedIn: ${marker}`));
  }
  if (login?.includes("spawn_account_hydration")) failures.push(sourceContractFailure(rule, "login handler spawns account hydration"));
  if (!promotion?.includes("spawn_account_hydration")) failures.push(sourceContractFailure(rule, "trust promotion does not spawn account hydration"));
  return failures;
}

export function checkCoreAccountHydrationGenerationFence() {
  const rule = "core.account.hydration_generation_fence";
  const actor = accountProductionSource("actor.rs");
  const profile = accountProductionSource("profile.rs");
  const failures = [];
  if (!actor.includes("AccountHydrationLoaded {")) failures.push(sourceContractFailure(rule, "account hydration does not return through the actor mailbox"));
  if (!profile.includes("generation != self.account_hydration_generation")) failures.push(sourceContractFailure(rule, "account hydration lacks its generation fence"));
  if (!profile.includes("fn invalidate_account_hydration(&mut self)")) failures.push(sourceContractFailure(rule, "account hydration invalidation helper is missing"));
  return failures;
}

export function checkCoreAccountAliasFailureReconciliation() {
  const rule = "core.account.alias_failure_reconciliation";
  const body = accountItemBody("profile.rs", "async fn handle_set_local_user_alias");
  const failures = [];
  if (!body?.includes("local_user_aliases_action_from_session(session).await")) failures.push(sourceContractFailure(rule, "alias failure does not reload authoritative aliases"));
  for (const marker of ["AppAction::LocalUserAliasUpdateFailed", "AppAction::LocalUserAliasesLoaded"]) if (!body?.includes(marker)) failures.push(sourceContractFailure(rule, `alias failure reconciliation lacks ${marker}`));
  return failures;
}

export function checkCoreAccountSecureBackupMonitorOwner() {
  const rule = "core.account.secure_backup_monitor_owner";
  const recovery = accountProductionSource("recovery_backup.rs");
  const actor = accountProductionSource("actor.rs");
  const scheduler = accountItemBody("recovery_backup.rs", "fn schedule_secure_backup_monitor");
  const retire = accountItemBody("recovery_backup.rs", "fn retire_secure_backup_monitor");
  const inspection = accountItemBody("recovery_backup.rs", "fn start_secure_backup_inspection");
  const failures = [];
  for (const [source, marker] of [[recovery, "const SECURE_BACKUP_MONITOR_INTERVAL: Duration = Duration::from_secs(60);"], [actor, "secure_backup_monitor_task: Option<crate::executor::JoinHandle<()>>"], [retire, "secure_backup_monitor_task.take()"], [scheduler, "SECURE_BACKUP_MONITOR_INTERVAL"], [scheduler, "monitor_serial"], [inspection, "retire_secure_backup_monitor()"]]) if (!source?.includes(marker)) failures.push(sourceContractFailure(rule, `secure-backup monitor is missing ${marker}`));
  return failures;
}

export function checkCoreAccountE2eeTypedFailureClassification() {
  const rule = "core.account.e2ee_typed_failure_classification";
  const recovery = accountProductionSource("recovery_backup.rs");
  const failures = [];
  for (const marker of ["async fn handle_export_room_keys", "async fn handle_import_room_keys", "async fn handle_bootstrap_secure_backup", "async fn handle_change_secure_backup_passphrase"]) {
    const body = accountItemBody("recovery_backup.rs", marker);
    if (!body?.includes("classify_e2ee_trust_error(&error)")) failures.push(sourceContractFailure(rule, `${marker} does not preserve typed failure classification`));
    if (body?.includes("Err(_)")) failures.push(sourceContractFailure(rule, `${marker} erases typed errors before classification`));
  }
  if (!recovery.includes("InvalidPassphrase")) failures.push(sourceContractFailure(rule, "recovery source lacks InvalidPassphrase classification"));
  return failures;
}

export function checkCoreAccountRecoveryKeyHydrationOrder() {
  const rule = "core.account.recovery_key_hydration_order";
  const submit = accountItemBody("recovery_backup.rs", "async fn handle_submit_recovery");
  const complete = accountItemBody("recovery_backup.rs", "async fn complete_recovery_after_verified");
  const failures = [];
  if (!submit?.includes("koushi_sdk::recover_e2ee")) failures.push(sourceContractFailure(rule, "recovery submission does not recover the secret"));
  const request = complete?.indexOf("AppAction::RestoreKeyBackupRequested") ?? -1;
  const restore = complete?.indexOf("koushi_sdk::download_joined_room_keys_from_backup") ?? -1;
  if (request < 0 || restore < 0 || request >= restore) failures.push(sourceContractFailure(rule, "joined-room key hydration does not follow restore-state projection"));
  return failures;
}

export function checkCoreAccountCrawlerNotificationLatestWins() {
  const rule = "core.account.crawler_notification_latest_wins";
  const actor = accountProductionSource("actor.rs");
  const notification = actor.split("AccountMessage::NotifySearchCrawlerRoomsAvailable")[1]?.split("AccountMessage::CurrentDeviceTrustChanged")[0];
  const failures = [];
  if (!notification?.includes("self.pending_crawler_notification = Some")) failures.push(sourceContractFailure(rule, "crawler notification is not retained latest-wins"));
  if (!notification?.includes("self.flush_pending_crawler_notification();")) failures.push(sourceContractFailure(rule, "crawler notification is not flushed without blocking"));
  if (notification?.includes("notify_rooms_available(room_ids, settings).await")) failures.push(sourceContractFailure(rule, "crawler notification awaits background capacity"));
  return failures;
}

export function checkCoreAccountSyncStopRouting() {
  const rule = "core.account.sync_stop_routing";
  const body = accountItemBody("routing.rs", "async fn route_sync_command");
  const failures = [];
  const gate = body?.indexOf("!matches!(command, SyncCommand::Stop { .. })") ?? -1;
  const spawn = body?.indexOf("self.spawn_sync_actor(session.clone()).await") ?? -1;
  const noActor = body?.indexOf("action=no_sync_actor") ?? -1;
  if (gate < 0 || spawn < 0 || noActor < 0 || !(gate < spawn && spawn < noActor)) failures.push(sourceContractFailure(rule, "Sync Stop routing does not separate the missing-actor path"));
  return failures;
}

export function checkCoreAccountManualSyncOnceGuard() {
  const rule = "core.account.manual_sync_once_guard";
  const body = accountItemBody("routing.rs", "async fn route_sync_command");
  const failures = [];
  const guard = body?.indexOf("is_manual_sync_once(") ?? -1;
  const spawn = body?.indexOf("self.spawn_sync_actor(") ?? -1;
  const send = body?.indexOf("handle.send(SyncMessage::Command(command))") ?? -1;
  if (guard < 0 || spawn < 0 || send < 0 || !(guard < spawn && guard < send)) failures.push(sourceContractFailure(rule, "manual SyncOnce guard does not precede actor routing"));
  const guarded = guard >= 0 && spawn >= 0 ? body.slice(guard, spawn) : "";
  for (const marker of ["CoreFailure::SyncFailed", "SyncFailureKind::Internal", "return;"]) if (!guarded.includes(marker)) failures.push(sourceContractFailure(rule, `manual SyncOnce rejection lacks ${marker}`));
  return failures;
}

export function checkCoreAccountSessionEstablishedHandoff() {
  const rule = "core.account.session_established_handoff";
  const body = accountItemBody("runtime_children.rs", "async fn spawn_sync_actor");
  const failures = [];
  const handoff = body?.indexOf(".send(RoomMessage::SessionEstablished") ?? -1;
  if (handoff < 0 || !body?.slice(handoff).includes(".await")) failures.push(sourceContractFailure(rule, "RoomActor session handoff is not reliably awaited"));
  if (body?.includes("room_actor.try_send(RoomMessage::SessionEstablished")) failures.push(sourceContractFailure(rule, "RoomActor session handoff uses try_send"));
  return failures;
}

export function checkCoreAccountSecureBackupContentBarrier() {
  const rule = "core.account.secure_backup_content_barrier";
  const cases = [
    ["routing.rs", "async fn route_timeline_command_with_permit_and_formatting_options"],
    ["scheduled_send.rs", "async fn handle_schedule_server_delayed_send"],
    ["scheduled_send.rs", "async fn handle_dispatch_local_scheduled_send"],
    ["scheduled_send.rs", "async fn handle_reschedule_server_delayed_send"]
  ];
  const failures = [];
  for (const [file, marker] of cases) if (!accountItemBody(file, marker)?.includes("admit_secure_backup_user_content")) failures.push(sourceContractFailure(rule, `${marker} lacks the secure-backup barrier`));
  const reschedule = accountItemBody("scheduled_send.rs", "async fn handle_reschedule_server_delayed_send");
  const barrier = reschedule?.indexOf("admit_secure_backup_user_content") ?? -1;
  const cancel = reschedule?.indexOf("UpdateAction::Cancel") ?? -1;
  if (barrier < 0 || cancel < 0 || barrier >= cancel) failures.push(sourceContractFailure(rule, "reschedule cancels before secure-backup admission"));
  return failures;
}

export function checkCoreAccountLocalScheduledSendNoBackupFence() {
  const rule = "core.account.local_scheduled_send_no_backup_fence";
  const body = accountItemBody("scheduled_send.rs", "async fn handle_dispatch_local_scheduled_send");
  return body?.includes(".require_backed_up_session()")
    ? [sourceContractFailure(rule, "local scheduled send has a per-session backup durability fence")]
    : [];
}

export function checkCoreAccountExplicitLogoutTeardown() {
  const rule = "core.account.explicit_logout_teardown";
  const logout = accountItemBody("session_lifecycle.rs", "pub(super) async fn handle_logout");
  const continuation = accountItemBody("session_lifecycle.rs", "match pending.continuation");
  const failures = [];
  if (!logout?.includes("perform_logout(request_id, true, false)")) failures.push(sourceContractFailure(rule, "explicit logout does not select non-preserving teardown"));
  for (const marker of ["preserve_persistence", "forget_last_session_pointer_if_matches(key_id)", "clear_account_persistence(key_id)", "session_persistence_deleted"]) if (!continuation?.includes(marker)) failures.push(sourceContractFailure(rule, `logout continuation lacks ${marker}`));
  return failures;
}

export function checkCoreAccountRestoreEventCacheStatus() {
  const rule = "core.account.restore_event_cache_status";
  const restore = accountItemBody("session_lifecycle.rs", "async fn restore_into_store");
  const helper = accountItemBody("actor.rs", "fn emit_event_cache_status(");
  const prepare = accountItemBody("session_lifecycle.rs", "async fn prepare_store_backed_session");
  const compact = (value) => value?.replace(/\s/gu, "") ?? "";
  const body = compact(restore);
  const helperBody = compact(helper);
  const prepareBody = compact(prepare);
  const failures = [];
  const storeConfig = body.indexOf("self.store.existing_account_store_config(key_id)");
  const restoreCall = body.indexOf("koushi_sdk::restore_session_with_verified_store");
  const encryptedStore = body.indexOf("letencrypted_store=store_config.store_config.encrypted_at_rest_configured();");
  const prepareCall = body.indexOf("self.prepare_store_backed_session(&session,encrypted_store).await");
  const returnOk = body.lastIndexOf("Ok(session)");
  if ([storeConfig, restoreCall, encryptedStore, prepareCall, returnOk].some((position) => position < 0) || !(storeConfig < restoreCall && storeConfig < encryptedStore && restoreCall < prepareCall && encryptedStore < prepareCall && prepareCall < returnOk)) failures.push(sourceContractFailure(rule, "store-backed restore ordering is incomplete"));
  for (const marker of ["koushi_sdk::enable_event_cache(session).await", "self.emit_event_cache_status(encrypted_store,&event_cache_result);"]) if (!prepareBody.includes(marker)) failures.push(sourceContractFailure(rule, `store-backed preparation lacks ${marker}`));
  for (const marker of ["EventCacheSubscribeStatus::Enabled,None", "EventCacheSubscribeStatus::AlreadyEnabled,None", "EventCacheSubscribeStatus::SubscribeFailed,Some(EventCacheFailureReasonClass::SubscribeFailed),"]) if (!helperBody.includes(marker)) failures.push(sourceContractFailure(rule, `event-cache diagnostic lacks ${marker}`));
  if ((prepareBody.match(/self\.emit_event_cache_status\(encrypted_store,&event_cache_result\);/gu) ?? []).length !== 1) failures.push(sourceContractFailure(rule, "event-cache status is not emitted exactly once"));
  for (const marker of ["enable_event_cache(&session).await.map_err", "enable_event_cache(&session).await?", "encrypted_store:true", "cache_path().is_some()"]) if (body.includes(marker) || helperBody.includes(marker)) failures.push(sourceContractFailure(rule, `restore event-cache path contains forbidden ${marker}`));
  return failures;
}

export function checkCoreAccountHomeserverChangeLoginAbort() {
  const rule = "core.account.homeserver_change_login_abort";
  const logout = accountItemBody("session_lifecycle.rs", "async fn perform_logout");
  const abort = accountItemBody("session_lifecycle.rs", "async fn abort_login");
  const failures = [];
  if (!logout?.includes("self.abort_login(login_session, &key_id, false, server_logout)")) failures.push(sourceContractFailure(rule, "logout does not pass server_logout to login abort"));
  for (const marker of ["if server_logout", "koushi_sdk::logout"]) if (!abort?.includes(marker)) failures.push(sourceContractFailure(rule, `login abort lacks ${marker}`));
  return failures;
}

export function checkCoreAccountAuthenticationQuarantine() {
  const rule = "core.account.authentication_quarantine";
  const password = accountItemBody("session_lifecycle.rs", "async fn handle_login_password");
  const restore = accountItemBody("session_lifecycle.rs", "async fn restore_account");
  const continuation = accountItemBody("sliding_sync.rs", "async fn continue_sliding_sync_admission");
  const completion = accountItemBody("sliding_sync.rs", "async fn finish_sliding_sync_capability_discovery");
  const failures = [];
  const before = password?.split("AppAction::LoginSucceeded")[0] ?? "";
  for (const marker of ["begin_sliding_sync_capability_discovery"]) if (!before.includes(marker)) failures.push(sourceContractFailure(rule, `password login lacks ${marker}`));
  for (const marker of ["persist_session(", "spawn_sync_actor(", "install_provisional_session"]) if (before.includes(marker)) failures.push(sourceContractFailure(rule, `password login performs premature ${marker}`));
  const beforeRestore = restore?.split("AppAction::RestoreSessionSucceeded")[0] ?? "";
  if (!beforeRestore.includes("begin_sliding_sync_capability_discovery")) failures.push(sourceContractFailure(rule, "restore lacks capability discovery"));
  for (const marker of ["spawn_sync_actor(", "install_provisional_session"]) if (beforeRestore.includes(marker)) failures.push(sourceContractFailure(rule, `restore performs premature ${marker}`));
  if (!continuation?.includes("install_provisional_session")) failures.push(sourceContractFailure(rule, "sliding-sync admission does not install the provisional session"));
  if (completion?.includes("self.continue_sliding_sync_admission(")) failures.push(sourceContractFailure(rule, "capability completion bypasses reducer continuation"));
  return failures;
}

export function checkCoreAccountRestoreTrace() {
  const rule = "core.account.restore_trace";
  const restoreLast = accountItemBody("session_lifecycle.rs", "async fn handle_restore_last_session");
  const restore = accountItemBody("session_lifecycle.rs", "async fn restore_account");
  const continuation = accountItemBody("sliding_sync.rs", "async fn continue_sliding_sync_admission");
  const actor = accountProductionSource("actor.rs");
  const failures = [];
  for (const marker of ["trace_account_request(\"restore_last_session\", request_id, \"load_pointer\")", "executor::spawn_blocking", "trace_account_request(\"restore_last_session\", request_id, \"pointer_found\")"]) if (!restoreLast?.includes(marker)) failures.push(sourceContractFailure(rule, `startup restore lacks ${marker}`));
  if (!restore?.includes("trace_account_request(\"restore_account\", request_id, \"load_session\")")) failures.push(sourceContractFailure(rule, "restore lacks load-session trace"));
  for (const marker of ["trace_account_request(", "\"restore_account\"", "core_request_id", "\"store_restore_ok\"", "install_provisional_session"]) if (!continuation?.includes(marker)) failures.push(sourceContractFailure(rule, `restore continuation lacks ${marker}`));
  if (restore?.includes("sync_actor_spawned")) failures.push(sourceContractFailure(rule, "restore reports sync actor spawn"));
  if (!actor.includes("DiagnosticField::request_id")) failures.push(sourceContractFailure(rule, "account diagnostics lack request correlation"));
  for (const source of [restoreLast, restore, continuation]) if (source?.includes("account_name()")) failures.push(sourceContractFailure(rule, "restore diagnostics expose an account identifier"));
  return failures;
}

export function checkCoreAccountRestoreDiagnostics() {
  const rule = "core.account.restore_diagnostics";
  const restore = accountItemBody("session_lifecycle.rs", "async fn restore_into_store");
  const recovery = accountItemBody("recovery_backup.rs", "async fn handle_recovery_finished");
  const promotion = accountItemBody("trust_gate.rs", "async fn promote_recovered_session_runtime");
  const failures = [];
  for (const marker of ["\"store_config_ready\"", "\"sdk_restore_begin\"", "\"sdk_restore_ok\""]) if (!restore?.includes(marker)) failures.push(sourceContractFailure(rule, `restore diagnostics lack ${marker}`));
  if (!recovery?.includes("\"post_recovery_trust_read\"")) failures.push(sourceContractFailure(rule, "recovery diagnostics lack post-recovery trust read"));
  for (const marker of ["\"persisted\"", "\"promoted\"", "current_device_trust_token"]) if (!promotion?.includes(marker)) failures.push(sourceContractFailure(rule, `recovery promotion diagnostics lack ${marker}`));
  return failures;
}

export function checkCoreAccountPasswordStoreFirst() {
  const rule = "core.account.password_store_first";
  const login = accountItemBody("session_lifecycle.rs", "async fn handle_login_password");
  const failures = [];
  for (const marker of ["Homeserver::parse", "existing_account_store_config", "pending_login_owner()", "login_with_password_with_new_device", "login_with_password_with_store_and_device"]) if (!login?.includes(marker)) failures.push(sourceContractFailure(rule, `password login lacks ${marker}`));
  for (const marker of ["login_with_existing_device", "fallback_to_fresh_device"]) if (login?.includes(marker)) failures.push(sourceContractFailure(rule, `password login contains forbidden ${marker}`));
  return failures;
}

export function checkCoreAccountSessionChangeObserver() {
  const rule = "core.account.session_change_observer";
  const start = accountItemBody("session_lifecycle.rs", "fn start_session_change_observer");
  const run = accountItemBody("session_lifecycle.rs", "async fn run_session_change_observation");
  const handler = accountItemBody("session_lifecycle.rs", "async fn handle_session_invalidated");
  const failures = [];
  for (const [body, marker] of [[start, "subscribe_to_session_changes()"], [run, "matrix_sdk::SessionChange::UnknownToken(data)"], [run, "soft_logout: data.soft_logout"], [handler, "AppAction::SessionAuthenticationInvalidated"], [handler, "self.stop_current_session_runtime().await"]]) if (!body?.includes(marker)) failures.push(sourceContractFailure(rule, `session-change observer lacks ${marker}`));
  return failures;
}

export function checkCoreAccountSoftLogoutReauth() {
  const rule = "core.account.soft_logout_reauth";
  const reauth = accountItemBody("session_lifecycle.rs", "async fn handle_soft_logout_reauth");
  const logout = accountItemBody("session_lifecycle.rs", "async fn perform_logout");
  const failures = [];
  const positions = ["drop(self.session.take())", "preflight_saved_crypto_store", "login_with_password_with_store_and_device"].map((marker) => reauth?.indexOf(marker) ?? -1);
  if (positions.some((position) => position < 0) || !(positions[0] < positions[1] && positions[1] < positions[2])) failures.push(sourceContractFailure(rule, "reauth does not retire, preflight, and replace in order"));
  for (const marker of ["locked_session_record = Some", "prepare_store_backed_session(&login_session, true)"]) if (!reauth?.includes(marker)) failures.push(sourceContractFailure(rule, `reauth lacks ${marker}`));
  for (const marker of ["locked_session_record.take()", "AppAction::LogoutFinished"]) if (!logout?.includes(marker)) failures.push(sourceContractFailure(rule, `logout lacks ${marker}`));
  return failures;
}

export function checkCoreAccountCredentialStoreBlocking() {
  const rule = "core.account.credential_store_blocking";
  const cases = [
    ["session_lifecycle.rs", "async fn persist_session"],
    ["session_lifecycle.rs", "async fn clear_account_persistence"],
    ["session_lifecycle.rs", "async fn lookup_session_key_id"],
    ["session_lifecycle.rs", "async fn handle_query_saved_sessions"],
    ["local_data_cleanup.rs", "async fn handle_probe_local_encryption_health"]
  ];
  const failures = [];
  for (const [file, marker] of cases) if (!accountItemBody(file, marker)?.includes("executor::spawn_blocking")) failures.push(sourceContractFailure(rule, `${marker} does not use the blocking port`));
  return failures;
}

export function checkCoreAccountSecureBackupLatch() {
  const rule = "core.account.secure_backup_latch";
  const inspection = accountItemBody("recovery_backup.rs", "fn start_secure_backup_inspection");
  const stateChange = accountItemBody("recovery_backup.rs", "async fn handle_secure_backup_state_changed");
  const teardown = accountItemBody("runtime_children.rs", "async fn stop_current_session_runtime");
  const completion = accountItemBody("recovery_backup.rs", "async fn finish_secure_backup_inspection");
  const failures = [];
  if (inspection?.includes("set_secure_backup_send_admitted(false)")) failures.push(sourceContractFailure(rule, "periodic backup inspection closes established admission"));
  for (const [body, marker] of [[stateChange, "set_secure_backup_send_admitted(false)"], [teardown, "set_secure_backup_send_admitted(false)"], [completion, "set_secure_backup_send_admitted(admitted)"]]) if (!body?.includes(marker)) failures.push(sourceContractFailure(rule, `backup latch lacks ${marker}`));
  return failures;
}

export function checkCoreAccountSessionStatusRefreshTeardown() {
  const rule = "core.account.session_status_refresh_teardown";
  const body = accountItemBody("runtime_children.rs", "async fn stop_current_session_runtime");
  return body?.includes("cancel_current_session_status_refresh().await") ? [] : [sourceContractFailure(rule, "runtime teardown does not cancel session-status refresh")];
}

export function checkCoreAccountProvisionalSyncRetry() {
  const rule = "core.account.provisional_sync_retry";
  const owner = accountSection("trust_gate.rs", "fn start_provisional_encryption_sync", "pub(super) async fn stop_provisional_encryption_sync");
  const failures = [];
  const branch = owner?.indexOf("if !first_response_seen.load(Ordering::Acquire)") ?? -1;
  const sleep = owner?.indexOf("executor::sleep(Duration::from_millis(250)).await;") ?? -1;
  const continueAt = sleep >= 0 ? owner.indexOf("continue;", sleep) : -1;
  const failed = continueAt >= 0 ? owner.indexOf("AccountMessage::ProvisionalEncryptionSyncFailed", continueAt) : -1;
  if (branch < 0 || sleep < 0 || continueAt < 0 || failed < 0 || !(branch < sleep && sleep < continueAt && continueAt < failed)) failures.push(sourceContractFailure(rule, "provisional sync retry does not remain under its owner before terminal failure"));
  return failures;
}

export function checkCoreAccountProvisionalSyncFirstResponse() {
  const rule = "core.account.provisional_sync_first_response";
  const owner = accountSection("trust_gate.rs", "fn start_provisional_encryption_sync", "pub(super) async fn stop_provisional_encryption_sync");
  const send = owner?.indexOf("callback_tx.send(message).await.is_ok()") ?? -1;
  const publish = owner?.indexOf("callback_first_response_seen.store(true, Ordering::Release)") ?? -1;
  return send >= 0 && publish >= 0 && send < publish ? [] : [sourceContractFailure(rule, "provisional first-response publication precedes actor delivery")];
}

export function checkCoreAccountAdmissionTimeoutTeardown() {
  const rule = "core.account.admission_timeout_teardown";
  const body = accountItemBody("session_lifecycle.rs", "pub(super) async fn stop_provisional_runtime");
  return body?.includes("cancel_verification_method_discovery_admission_timeout()") ? [] : [sourceContractFailure(rule, "provisional runtime teardown does not cancel admission timeout")];
}

export function checkCoreAccountProvisionalEncryptionSyncService() {
  const rule = "core.account.provisional_encryption_sync_service";
  const body = accountItemBody("trust_gate.rs", "fn start_provisional_encryption_sync");
  const failures = [];
  if (!body?.includes("provisional_encryption_sync_loop")) failures.push(sourceContractFailure(rule, "provisional verification lacks EncryptionSyncService loop"));
  if (body?.includes("restricted_verification_sync_once_with_token")) failures.push(sourceContractFailure(rule, "provisional verification constructs classic sync"));
  return failures;
}

export function checkCoreAccountQaDeviceKeyRefresh() {
  const rule = "core.account.qa_device_key_refresh";
  const helper = accountItemBody("trust_gate.rs", "async fn refresh_device_keys_and_assert_known");
  const query = helper?.indexOf("request_user_identity(&user_id)") ?? -1;
  const device = helper?.indexOf("get_device(&user_id, &device_id)") ?? -1;
  const failures = [];
  if (query < 0 || device < 0 || query >= device) failures.push(sourceContractFailure(rule, "QA device refresh does not query before exact-device assertion"));
  if (device < 0 || !helper?.slice(device).includes(".ok_or(())?")) failures.push(sourceContractFailure(rule, "QA device refresh does not require the exact device"));
  return failures;
}

export function checkCoreAccountVerificationDiscoveryCompletion() {
  const rule = "core.account.verification_discovery_completion";
  const actor = accountProductionSource("actor.rs");
  const completion = actor.split("AccountMessage::VerificationMethodsDiscovered")[1]?.split("AccountMessage::RecoveryFinished")[0];
  const failures = [];
  if (completion?.includes("owned.task.await")) failures.push(sourceContractFailure(rule, "verification discovery completion awaits its sender task"));
  if (!completion?.includes("success_projected")) failures.push(sourceContractFailure(rule, "verification discovery completion lacks success projection diagnostic"));
  return failures;
}

export function checkCoreAccountSasAdoption() {
  const rule = "core.account.sas_adoption";
  const body = accountItemBody("verification.rs", "async fn store_sas_verification(");
  const failures = [];
  const classify = body?.indexOf("resolve_sas_adoption(") ?? -1;
  const earlyReturn = body?.indexOf("return;") ?? -1;
  if (classify < 0 || earlyReturn < 0 || classify >= earlyReturn) failures.push(sourceContractFailure(rule, "SAS adoption classifies after its early return"));
  if (!body?.includes("koushi_sdk::cancel_sas_verification(&handle)")) failures.push(sourceContractFailure(rule, "conflicting SAS handle is not cancelled"));
  for (const marker of ["self.stop_sas_verification_observer().await", "self.sas_verification = Some", "self.start_sas_timeout(", "self.observe_sas_verification(", "koushi_sdk::accept_sas_verification("]) {
    const position = body?.indexOf(marker) ?? -1;
    if (position < 0 || position <= earlyReturn) failures.push(sourceContractFailure(rule, `SAS adoption guard does not precede ${marker}`));
  }
  return failures;
}

export function checkCoreAccountIncomingVerificationAdmission() {
  const rule = "core.account.incoming_verification_admission";
  const body = accountItemBody("verification.rs", "async fn handle_incoming_verification_request");
  const failures = [];
  const positions = ["own_user_active: self.own_user_verification.is_some()", "match decision", "koushi_sdk::cancel_verification_request(&handle).await", "self.verification_request = Some", "self.observe_verification_request("].map((marker) => body?.indexOf(marker) ?? -1);
  if (positions.some((position) => position < 0) || positions[0] >= positions[1] || positions[2] >= positions[3] || positions[2] >= positions[4]) failures.push(sourceContractFailure(rule, "incoming verification admission ordering is incomplete"));
  return failures;
}

export function checkCoreAccountIdentityResetAuthLifecycle() {
  const rule = "core.account.identity_reset_auth_lifecycle";
  const fields = accountItemBody("actor.rs", "pub struct AccountActor {");
  const route = accountItemBody("actor.rs", "async fn handle_command");
  const cancel = accountItemBody("verification.rs", "async fn handle_cancel_identity_reset");
  const required = accountItemBody("recovery_backup.rs", "IdentityResetOutcome::AuthRequired(handle)");
  const timeout = accountItemBody("verification.rs", "async fn handle_identity_reset_auth_timeout");
  const cleanup = accountItemBody("verification.rs", "async fn cancel_identity_reset_handle");
  const failures = [];
  for (const [body, marker] of [[fields, "identity_reset_timeout_task"], [route, "AccountCommand::CancelIdentityReset"], [cancel, "AppAction::ResetIdentityCancelled"], [required, "spawn_identity_reset_auth_timeout"], [timeout, "AppAction::ResetIdentityTimedOut"], [cleanup, "identity_reset_timeout_task"]]) if (!body?.includes(marker)) failures.push(sourceContractFailure(rule, `identity-reset lifecycle lacks ${marker}`));
  return failures;
}

export function runSourceContractRules() {
  return [
    checkCoreAccountSessionReplacementTeardown(),
    checkCoreAccountReliableReducerDelivery(),
    checkCoreAccountLoginHydrationOrder(),
    checkCoreAccountHydrationGenerationFence(),
    checkCoreAccountAliasFailureReconciliation(),
    checkCoreAccountSecureBackupMonitorOwner(),
    checkCoreAccountE2eeTypedFailureClassification(),
    checkCoreAccountRecoveryKeyHydrationOrder(),
    checkCoreAccountCrawlerNotificationLatestWins(),
    checkCoreAccountSyncStopRouting(),
    checkCoreAccountManualSyncOnceGuard(),
    checkCoreAccountSessionEstablishedHandoff(),
    checkCoreAccountSecureBackupContentBarrier(),
    checkCoreAccountLocalScheduledSendNoBackupFence(),
    checkCoreAccountExplicitLogoutTeardown(),
    checkCoreAccountRestoreEventCacheStatus(),
    checkCoreAccountHomeserverChangeLoginAbort(),
    checkCoreAccountAuthenticationQuarantine(),
    checkCoreAccountRestoreTrace(),
    checkCoreAccountRestoreDiagnostics(),
    checkCoreAccountPasswordStoreFirst(),
    checkCoreAccountSessionChangeObserver(),
    checkCoreAccountSoftLogoutReauth(),
    checkCoreAccountCredentialStoreBlocking(),
    checkCoreAccountSecureBackupLatch(),
    checkCoreAccountSessionStatusRefreshTeardown(),
    checkCoreAccountProvisionalSyncRetry(),
    checkCoreAccountProvisionalSyncFirstResponse(),
    checkCoreAccountAdmissionTimeoutTeardown(),
    checkCoreAccountProvisionalEncryptionSyncService(),
    checkCoreAccountQaDeviceKeyRefresh(),
    checkCoreAccountVerificationDiscoveryCompletion(),
    checkCoreAccountSasAdoption(),
    checkCoreAccountIncomingVerificationAdmission(),
    checkCoreAccountIdentityResetAuthLifecycle(),
    checkStateFocusedContextReducerContract(),
    checkStateHasNoLegacySyncModeVocabulary(),
    checkSdkPasswordSmokeRuntimeSafety(),
    checkSdkClientStoreConfigContract(),
    checkSdkDesktopClientBuilderDefaults(),
    checkSdkBackupDownloadDefault(),
    checkSdkRecoveryUsesSdkSignaturePublication(),
    checkSdkRecoverySignatureRoundTripContract(),
    checkSdkRoomReadMarkerContract(),
    checkSdkSpaceInviteCancellationContract(),
    checkSdkRoomTagMethods(),
    checkSdkPinnedEventMethods(),
    checkSdkRoomManagementMethods(),
    checkSdkJoinedRoomListDirectDetection(),
    checkSdkJoinedRoomListAvoidsFullMemberScans(),
    checkSdkDmResolutionCandidates(),
    checkSdkSpaceMemberIdsNoSync(),
    checkSdkJoinedOnlySpaceMemberProjection(),
    checkSdkSpaceLookupFailuresPropagate(),
    checkSdkFailedSpaceMemberCountsUnavailable(),
    checkSdkRoomMemberSummariesUseFullMembers(),
    checkSdkDirectAccountDataLoaderIsLocalOnly(),
    checkSdkDirectAccountDataServerFallback(),
    checkSdkSlidingSyncInviteProbeContract(),
    checkSdkSessionBackupFence(),
    checkSdkLibrarySourceManifest(),
    checkSdkCommittedRoomCheckpointHasNoLegacyApi(),
    checkDesktopTauriCommandRegistrationContract(),
    checkDesktopSubmitCoreCommandContract(),
    checkDesktopEventWaitLagContract(),
    checkDesktopFailureWaiterContract(),
    checkDesktopActivityNavigationContract(),
    checkDesktopActivityCommandContract(),
    checkDesktopLoginWaitContract(),
    checkDesktopE2eeCommandContract(),
    checkDesktopLocalEncryptionCommandContract(),
    checkDesktopProfileCommandContract(),
    checkDesktopDirectoryStartDmContract(),
    checkDesktopDirectoryJoinRoomContract(),
    checkDesktopRoomOperationContract(),
    checkDesktopSpaceOperationContract(),
    checkDesktopSearchCommandContract(),
    checkDesktopSettingsCommandContract(),
    checkDesktopNavigationContract(),
    checkDesktopSpaceTraceContract(),
    checkDesktopTimelineGenerationAckContract(),
    checkDesktopTimelineCommandContract(),
    checkDesktopTimelineSignalContract(),
    checkDesktopScheduledSendCommandContract(),
    checkDesktopSendQueueCommandContract(),
    checkDesktopForwarderLagRecoveryContract(),
    checkDesktopQaControlPipeContract(),
    checkDesktopNativeWindowLifecycleContract(),
    checkDesktopNativeReopenContract(),
    checkDesktopViewportAdapterIsolationContract()
  ].flat();
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
  if (violation.kind === "source-contract") return `${violation.rule}: ${violation.message}`;
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
  result.violations.push(...runSourceContractRules());
  if (inventory) {
    process.stdout.write(inventoryReport(result));
  } else if (result.violations.length > 0) {
    console.error("Rust test structure violations:");
    for (const violation of result.violations) console.error(`- ${formatViolation(violation)}`);
    process.exitCode = 1;
  }
}
