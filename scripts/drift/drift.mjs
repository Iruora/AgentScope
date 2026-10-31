#!/usr/bin/env node
// Checks the latest Claude Code, Codex and Copilot CLI releases against what
// AgentScope's adapters were tested with, and (with --issues) keeps one GitHub
// issue per harness up to date. Run weekly by .github/workflows/drift.yml.
//
//   node scripts/drift/drift.mjs            print a report
//   node scripts/drift/drift.mjs --issues   also open, update or close issues (needs gh)
//
// Two kinds of drift are reported:
// - a newer major.minor than `tested` in harnesses.json, and
// - hook event names the adapter installs that no longer appear in the
//   harness's binary (a likely rename or removal, which breaks the adapter).
// Cursor is not on npm, so it is not checked here.

import { execFileSync } from "node:child_process";
import { closeSync, mkdtempSync, openSync, readFileSync, readSync, rmSync, appendFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const LABEL = "harness-drift";
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

/** Event names from an adapter's `pub const EVENTS: &[&str] = &[...]`. */
export function parseEvents(rustSource) {
  const block = rustSource.match(/pub const EVENTS: &\[&str\] = &\[([\s\S]*?)\];/);
  if (!block) throw new Error("no `pub const EVENTS` list found");
  return [...block[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

/** True when `latest` is a newer release line than `tested` ("2.1" or "0.165"). */
export function isNewerLine(latest, tested) {
  const a = latest.split(/[.-]/).map(Number);
  const b = tested.split(".").map(Number);
  for (let i = 0; i < b.length; i++) {
    if ((a[i] ?? 0) !== b[i]) return (a[i] ?? 0) > b[i];
  }
  return false;
}

/** Which of `needles` occur in the file, read in chunks so large binaries stay cheap. */
export function scanFile(path, needles) {
  const wanted = needles.map((n) => ({ name: n, bytes: Buffer.from(n) }));
  const overlap = Math.max(...wanted.map((w) => w.bytes.length)) - 1;
  const found = new Set();
  const fd = openSync(path, "r");
  try {
    const chunk = Buffer.alloc(8 * 1024 * 1024);
    let carry = Buffer.alloc(0);
    for (;;) {
      const n = readSync(fd, chunk, 0, chunk.length, null);
      if (n === 0) break;
      const window = Buffer.concat([carry, chunk.subarray(0, n)]);
      for (const w of wanted) if (!found.has(w.name) && window.includes(w.bytes)) found.add(w.name);
      if (found.size === wanted.length) break;
      carry = window.subarray(Math.max(0, window.length - overlap));
    }
  } finally {
    closeSync(fd);
  }
  return needles.filter((n) => found.has(n));
}

const npm = (args, opts = {}) =>
  execFileSync("npm", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], ...opts }).trim();

const defaultTools = {
  latestVersion: (pkg) => npm(["view", pkg, "version"]),
  /** Downloads and unpacks an npm package; returns its directory and a cleanup function. */
  unpack: (spec) => {
    const dir = mkdtempSync(join(tmpdir(), "agentscope-drift-"));
    const tarball = npm(["pack", spec, "--silent", "--pack-destination", dir]).split("\n").pop();
    execFileSync("tar", ["xzf", join(dir, tarball), "-C", dir]);
    rmSync(join(dir, tarball));
    return { dir, cleanup: () => rmSync(dir, { recursive: true, force: true }) };
  },
  readSource: (path) => readFileSync(join(ROOT, path), "utf8"),
};

/** Checks one harness. Never throws: failures come back as status "error". */
export function check(id, cfg, tools = defaultTools) {
  const result = { id, name: cfg.name, package: cfg.package, tested: cfg.tested, missing: [], checked: [] };
  try {
    result.latest = tools.latestVersion(cfg.package);
    result.newerLine = isNewerLine(result.latest, cfg.tested);
    const events = parseEvents(tools.readSource(cfg.events));
    if (cfg.scan) {
      const pkg = tools.unpack(cfg.scan.spec.replace("{version}", result.latest));
      try {
        const present = scanFile(join(pkg.dir, cfg.scan.file), events);
        result.checked = events;
        result.missing = events.filter((e) => !present.includes(e));
      } finally {
        pkg.cleanup();
      }
    } else {
      result.scanNote = cfg.scanNote;
    }
    result.status = result.missing.length ? "missing-events" : result.newerLine ? "new-version" : "ok";
  } catch (err) {
    result.status = "error";
    result.error = String(err?.stderr || err?.message || err).trim().split("\n").slice(0, 5).join("\n");
  }
  return result;
}

export const issueTitle = (r) => `Harness drift: ${r.name}`;

export function issueBody(r) {
  const lines = [
    `<!-- agentscope-drift:${r.id}:${r.latest} -->`,
    `The weekly drift check found a change in **${r.name}** (\`${r.package}\`).`,
    "",
    `- Latest release: **${r.latest}**`,
    `- Tested with: **${r.tested}.x** (\`tested\` in \`scripts/drift/harnesses.json\`)`,
  ];
  if (r.missing.length) {
    lines.push(
      "",
      `**These hook events the adapter installs no longer appear in ${r.latest}:** ${r.missing.map((e) => `\`${e}\``).join(", ")}.`,
      "They were probably renamed or removed, so AgentScope may stop receiving them. Check the harness's hook docs and changelog, then update the adapter and its fixtures.",
    );
  } else if (r.checked.length) {
    lines.push("", `All ${r.checked.length} hook events the adapter installs still appear in the new binary, so this is most likely safe.`);
  } else if (r.scanNote) {
    lines.push("", r.scanNote);
  }
  lines.push(
    "",
    "**To clear this issue:** check the changelog for hook or transcript changes, update the adapter and fixtures if needed, then set `tested` to the new release line. The next run closes this issue on its own.",
  );
  return lines.join("\n");
}

export function summary(results) {
  const icon = { ok: "✅", "new-version": "🆕", "missing-events": "⚠️", error: "❌" };
  const rows = results.map((r) => {
    const detail =
      r.status === "error" ? r.error.split("\n")[0]
      : r.missing.length ? `missing: ${r.missing.join(", ")}`
      : r.checked.length ? `${r.checked.length} events found`
      : "version only";
    return `| ${icon[r.status]} ${r.name} | ${r.latest ?? "?"} | ${r.tested}.x | ${detail} |`;
  });
  return [
    "## Harness drift",
    "",
    "| Harness | Latest | Tested | Hook events |",
    "|---|---|---|---|",
    ...rows,
    "",
    "Cursor is not published on npm and is not checked automatically.",
  ].join("\n");
}

const gh = (args, input) =>
  execFileSync("gh", args, { encoding: "utf8", input, stdio: [input ? "pipe" : "ignore", "pipe", "inherit"] }).trim();

/** Opens, updates (silently, by editing the body) or closes the harness's issue. */
export function syncIssue(r, open, run = gh) {
  const existing = open.find((i) => i.title === issueTitle(r));
  if (r.status === "error") return "skipped";
  if (r.status === "ok") {
    if (!existing) return "none";
    run(["issue", "close", String(existing.number), "--comment", `${r.name} ${r.latest} matches the tested line ${r.tested}.x again. Closing.`]);
    return "closed";
  }
  const body = issueBody(r);
  if (!existing) {
    run(["issue", "create", "--title", issueTitle(r), "--label", LABEL, "--body-file", "-"], body);
    return "opened";
  }
  if (existing.body.trim() === body.trim()) return "unchanged";
  run(["issue", "edit", String(existing.number), "--body-file", "-"], body);
  return "updated";
}

function main() {
  const withIssues = process.argv.includes("--issues");
  const config = JSON.parse(readFileSync(join(ROOT, "scripts/drift/harnesses.json"), "utf8"));
  const results = Object.entries(config).map(([id, cfg]) => check(id, cfg));
  const report = summary(results);
  console.log(report);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, report + "\n");

  if (withIssues) {
    gh(["label", "create", LABEL, "--color", "0891b2", "--description", "A harness changed since AgentScope was tested", "--force"]);
    const open = JSON.parse(gh(["issue", "list", "--label", LABEL, "--state", "open", "--json", "number,title,body", "--limit", "50"]));
    for (const r of results) console.log(`${r.name}: issue ${syncIssue(r, open)}`);
  }
  for (const r of results.filter((r) => r.status === "error")) console.error(`${r.name}: ${r.error}`);
  if (results.some((r) => r.status === "error")) process.exitCode = 1;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
