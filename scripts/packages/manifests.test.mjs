import assert from "node:assert/strict";
import { test } from "node:test";

import { allFiles, assetNames, parseSums } from "./manifests.mjs";

const version = "1.2.0";
const sums = Object.fromEntries(
  Object.values(assetNames(version)).map((name, i) => [name, String(i + 1).repeat(64)]),
);

test("parses sha256sum output", () => {
  const text = `${"a".repeat(64)}  dist/AgentScope_1.2.0_amd64.deb\n${"B".repeat(64)} *AgentScope_1.2.0_x64.dmg\nnoise\n`;
  assert.deepEqual(parseSums(text), {
    "AgentScope_1.2.0_amd64.deb": "a".repeat(64),
    "AgentScope_1.2.0_x64.dmg": "b".repeat(64),
  });
});

test("writes every manifest with the right hashes", () => {
  const files = allFiles(version, sums);
  assert.deepEqual(Object.keys(files).sort(), [
    "aur/.SRCINFO",
    "aur/PKGBUILD",
    "homebrew/Casks/agentscope.rb",
    "scoop/bucket/agentscope.json",
    "winget/Iruora.AgentScope.installer.yaml",
    "winget/Iruora.AgentScope.locale.en-US.yaml",
    "winget/Iruora.AgentScope.yaml",
  ]);
  const cask = files["homebrew/Casks/agentscope.rb"];
  assert.match(cask, /version "1\.2\.0"/);
  assert.match(cask, new RegExp(`arm:   "${sums["AgentScope_1.2.0_aarch64.dmg"]}"`));
  assert.match(cask, new RegExp(`intel: "${sums["AgentScope_1.2.0_x64.dmg"]}"`));

  const scoop = JSON.parse(files["scoop/bucket/agentscope.json"]);
  assert.equal(scoop.architecture["64bit"].hash, sums["AgentScope_1.2.0_x64_portable.zip"]);
  assert.equal(
    scoop.architecture["64bit"].url,
    "https://github.com/Iruora/AgentScope/releases/download/v1.2.0/AgentScope_1.2.0_x64_portable.zip",
  );

  const installer = files["winget/Iruora.AgentScope.installer.yaml"];
  assert.match(installer, new RegExp(sums["AgentScope_1.2.0_x64-setup.exe"].toUpperCase()));
  assert.match(installer, /AgentScope_1\.2\.0_x64_en-US\.msi/);

  assert.match(files["aur/PKGBUILD"], /^pkgver=1\.2\.0$/m);
  assert.match(files["aur/PKGBUILD"], /releases\/download\/v\$\{pkgver\}\/AgentScope_\$\{pkgver\}_amd64\.deb/);
  assert.match(files["aur/.SRCINFO"], new RegExp(`sha256sums_x86_64 = ${sums["AgentScope_1.2.0_amd64.deb"]}`));
});

test("refuses missing downloads and odd versions", () => {
  const partial = { ...sums };
  delete partial["AgentScope_1.2.0_amd64.deb"];
  assert.throws(() => allFiles(version, partial), /amd64\.deb/);
  assert.throws(() => allFiles("1.2.0-beta", sums), /not a release version/);
});
