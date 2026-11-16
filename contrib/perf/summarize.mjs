// Summarize one or more results.json files from bench.mjs (p50 and max per metric).
// Usage: node contrib/perf/summarize.mjs <run-dir>/results.json [...]
import { readFileSync } from "node:fs";
const all = process.argv.slice(2).flatMap((p) => JSON.parse(readFileSync(p, "utf8")).map((r) => ({ ...r, file: p })));
const q = (a, p) => { const s = [...a].sort((x, y) => x - y); return s.length ? s[Math.min(s.length - 1, Math.floor(p * s.length))] : null; };
const groups = {};
for (const r of all) {
  const key = r.name.startsWith("switch.") ? `${r.name}.${r.room}` : r.name;
  (groups[key] ??= []).push(r);
}
for (const [k, rs] of Object.entries(groups)) {
  const fields = new Set(rs.flatMap((r) => Object.keys(r).filter((f) => typeof r[f] === "number")));
  const parts = [];
  for (const f of fields) {
    const v = rs.map((r) => r[f]).filter((x) => typeof x === "number");
    parts.push(v.length > 1 ? `${f}: p50=${q(v, 0.5)} max=${Math.max(...v)}` : `${f}=${v[0]}`);
  }
  const flags = rs.filter((r) => r.timeout || r.error).map((r) => r.error ?? "timeout");
  console.log(`${k} (n=${rs.length})  ${parts.join("  ")}${flags.length ? "  FLAGS=" + flags.join(",") : ""}`);
}
