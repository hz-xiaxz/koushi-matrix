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

export function runSourceContractRules() {
  return [
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
    checkSdkCommittedRoomCheckpointHasNoLegacyApi()
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
