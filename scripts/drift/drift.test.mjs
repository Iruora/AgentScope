import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { check, isNewerLine, issueBody, issueTitle, parseEvents, scanFile, summary, syncIssue } from "./drift.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

test("parses every adapter's EVENTS list", () => {
  const config = JSON.parse(readFileSync(join(ROOT, "scripts/drift/harnesses.json"), "utf8"));
  for (const cfg of Object.values(config)) {
    const events = parseEvents(readFileSync(join(ROOT, cfg.events), "utf8"));
    assert.ok(events.length >= 5, `${cfg.events} has events`);
    assert.ok(events.every((e) => /^[A-Za-z]+$/.test(e)));
  }
});

test("compares release lines", () => {
  assert.equal(isNewerLine("2.1.316", "2.1"), false);
  assert.equal(isNewerLine("2.2.0", "2.1"), true);
  assert.equal(isNewerLine("3.0.0", "2.1"), true);
  assert.equal(isNewerLine("0.166.0", "0.165"), true);
  assert.equal(isNewerLine("0.164.2", "0.165"), false);
  assert.equal(isNewerLine("1.0.106-0", "1.0"), false);
});

test("finds needles across chunk boundaries", () => {
  const dir = mkdtempSync(join(tmpdir(), "drift-test-"));
  try {
    const file = join(dir, "bin");
    const filler = Buffer.alloc(8 * 1024 * 1024 - 4, 0x61);
    writeFileSync(file, Buffer.concat([filler, Buffer.from("SubagentStart\0PreToolUse")]));
    assert.deepEqual(scanFile(file, ["PreToolUse", "SubagentStart", "Gone"]), ["PreToolUse", "SubagentStart"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

const cfg = { name: "Demo", package: "demo", tested: "1.2", events: "x.rs", scan: { spec: "demo-bin@{version}", file: "bin" } };
const source = 'pub const EVENTS: &[&str] = &[\n    "Start",\n    "Stop",\n];';

function fakeTools(latest, binary) {
  const dir = mkdtempSync(join(tmpdir(), "drift-test-"));
  writeFileSync(join(dir, "bin"), binary);
  return {
    latestVersion: () => latest,
    readSource: () => source,
    unpack: (spec) => {
      assert.equal(spec, `demo-bin@${latest}`);
      return { dir, cleanup: () => rmSync(dir, { recursive: true, force: true }) };
    },
  };
}

test("reports ok, new versions and missing events", () => {
  assert.equal(check("demo", cfg, fakeTools("1.2.9", "Start Stop")).status, "ok");
  assert.equal(check("demo", cfg, fakeTools("1.3.0", "Start Stop")).status, "new-version");
  const r = check("demo", cfg, fakeTools("1.3.0", "Start Halt"));
  assert.equal(r.status, "missing-events");
  assert.deepEqual(r.missing, ["Stop"]);
  assert.match(issueBody(r), /`Stop`/);
});

test("errors are reported, not thrown", () => {
  const tools = { ...fakeTools("1.2.0", ""), latestVersion: () => { throw new Error("E404 not found"); } };
  const r = check("demo", cfg, tools);
  assert.equal(r.status, "error");
  assert.match(summary([r]), /E404/);
});

test("keeps one issue per harness", () => {
  const calls = [];
  const run = (args, input) => { calls.push([args[1], input]); return ""; };
  const drift = check("demo", cfg, fakeTools("1.3.0", "Start Stop"));

  assert.equal(syncIssue(drift, [], run), "opened");
  const open = [{ number: 7, title: issueTitle(drift), body: issueBody(drift) }];
  assert.equal(syncIssue(drift, open, run), "unchanged");
  assert.equal(syncIssue({ ...drift, latest: "1.3.1" }, open, run), "updated");
  assert.equal(syncIssue({ ...drift, status: "ok" }, open, run), "closed");
  assert.equal(syncIssue({ ...drift, status: "error" }, open, run), "skipped");
  assert.equal(syncIssue({ ...drift, status: "ok" }, [], run), "none");
  assert.deepEqual(calls.map((c) => c[0]), ["create", "edit", "close"]);
});
