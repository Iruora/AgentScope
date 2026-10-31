#!/usr/bin/env node
// Writes the package-manager manifests for one AgentScope release: a Homebrew
// cask, a Scoop manifest, winget manifests, and an AUR PKGBUILD with its
// .SRCINFO. Run by .github/workflows/packages.yml after a release is published.
//
//   node scripts/packages/manifests.mjs --version 1.2.0 --sums SHA256SUMS --out dir
//
// SHA256SUMS is `sha256sum` output for the release downloads.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const REPO = "Iruora/AgentScope";
const HOMEPAGE = `https://github.com/${REPO}`;
const DESCRIPTION = "See every local AI coding agent, its subagents, and which ones are waiting for your approval";
const WINGET_ID = "Iruora.AgentScope";

/** The download file names for a version, as the Release workflow names them. */
export function assetNames(version) {
  return {
    dmgArm: `AgentScope_${version}_aarch64.dmg`,
    dmgIntel: `AgentScope_${version}_x64.dmg`,
    setup: `AgentScope_${version}_x64-setup.exe`,
    msi: `AgentScope_${version}_x64_en-US.msi`,
    portable: `AgentScope_${version}_x64_portable.zip`,
    deb: `AgentScope_${version}_amd64.deb`,
  };
}

const download = (version, name) => `${HOMEPAGE}/releases/download/v${version}/${name}`;

/** Parses `sha256sum` output into { fileName: hash }. */
export function parseSums(text) {
  const sums = {};
  for (const line of text.split("\n")) {
    const m = line.trim().match(/^([0-9a-f]{64})\s+\*?(.+)$/i);
    if (m) sums[m[2].split("/").pop()] = m[1].toLowerCase();
  }
  return sums;
}

function hashes(version, sums) {
  const names = assetNames(version);
  const out = {};
  for (const [key, name] of Object.entries(names)) {
    if (!sums[name]) throw new Error(`no SHA-256 for ${name}`);
    out[key] = sums[name];
  }
  return out;
}

export function cask(version, h) {
  return `cask "agentscope" do
  arch arm: "aarch64", intel: "x64"

  version "${version}"
  sha256 arm:   "${h.dmgArm}",
         intel: "${h.dmgIntel}"

  url "${HOMEPAGE}/releases/download/v#{version}/AgentScope_#{version}_#{arch}.dmg"
  name "AgentScope"
  desc "${DESCRIPTION}"
  homepage "${HOMEPAGE}"

  livecheck do
    url :url
    strategy :github_latest
  end

  auto_updates true
  depends_on macos: ">= :big_sur"

  app "AgentScope.app"

  zap trash: [
    "~/Library/Application Support/AgentScope",
    "~/Library/Caches/io.github.iruora.agentscope",
    "~/Library/WebKit/io.github.iruora.agentscope",
  ]

  caveats <<~EOS
    AgentScope is not notarized yet. If macOS says it cannot verify the app, run:
      xattr -dr com.apple.quarantine "#{appdir}/AgentScope.app"
    Before uninstalling, disconnect each harness in Settings → Integrations.
  EOS
end
`;
}

export function scoop(version, h) {
  const manifest = {
    version,
    description: DESCRIPTION,
    homepage: HOMEPAGE,
    license: "MIT|Apache-2.0",
    notes: "Update with `scoop update agentscope`. This portable build does not update itself.",
    architecture: {
      "64bit": { url: download(version, assetNames(version).portable), hash: h.portable },
    },
    extract_dir: "AgentScope",
    shortcuts: [["AgentScope.exe", "AgentScope"]],
    checkver: "github",
    autoupdate: {
      architecture: {
        "64bit": { url: download("$version", "AgentScope_$version_x64_portable.zip") },
      },
    },
  };
  return JSON.stringify(manifest, null, 2) + "\n";
}

const yaml = (lines) => lines.join("\n") + "\n";
const WINGET_SCHEMA = "1.6.0";

export function winget(version, h) {
  const names = assetNames(version);
  const head = [`PackageIdentifier: ${WINGET_ID}`, `PackageVersion: ${version}`];
  return {
    [`${WINGET_ID}.yaml`]: yaml([
      ...head,
      "DefaultLocale: en-US",
      "ManifestType: version",
      `ManifestVersion: ${WINGET_SCHEMA}`,
    ]),
    [`${WINGET_ID}.installer.yaml`]: yaml([
      ...head,
      "MinimumOSVersion: 10.0.17763.0",
      "Installers:",
      "- Architecture: x64",
      "  InstallerType: nullsoft",
      "  Scope: user",
      `  InstallerUrl: ${download(version, names.setup)}`,
      `  InstallerSha256: ${h.setup.toUpperCase()}`,
      "- Architecture: x64",
      "  InstallerType: wix",
      "  Scope: machine",
      `  InstallerUrl: ${download(version, names.msi)}`,
      `  InstallerSha256: ${h.msi.toUpperCase()}`,
      "ManifestType: installer",
      `ManifestVersion: ${WINGET_SCHEMA}`,
    ]),
    [`${WINGET_ID}.locale.en-US.yaml`]: yaml([
      ...head,
      "PackageLocale: en-US",
      "Publisher: Iruora",
      `PublisherUrl: https://github.com/Iruora`,
      `PublisherSupportUrl: ${HOMEPAGE}/issues`,
      "PackageName: AgentScope",
      `PackageUrl: ${HOMEPAGE}`,
      "License: MIT OR Apache-2.0",
      `LicenseUrl: ${HOMEPAGE}#license`,
      `ShortDescription: ${DESCRIPTION}`,
      "Tags:",
      "- ai",
      "- agents",
      "- claude-code",
      "- codex",
      "- copilot",
      "- cursor",
      `ReleaseNotesUrl: ${HOMEPAGE}/releases/tag/v${version}`,
      "ManifestType: defaultLocale",
      `ManifestVersion: ${WINGET_SCHEMA}`,
    ]),
  };
}

const AUR = {
  pkgname: "agentscope-bin",
  depends: ["webkit2gtk-4.1", "gtk3", "libayatana-appindicator"],
  licenses: ["MIT", "Apache-2.0"],
};

export function pkgbuild(version, h) {
  const deb = assetNames(version).deb;
  return `# Maintainer: Iruora <https://github.com/Iruora>
# Generated by AgentScope's release workflow; edit scripts/packages/manifests.mjs instead.
pkgname=${AUR.pkgname}
pkgver=${version}
pkgrel=1
pkgdesc="${DESCRIPTION}"
arch=('x86_64')
url="${HOMEPAGE}"
license=(${AUR.licenses.map((l) => `'${l}'`).join(" ")})
depends=(${AUR.depends.map((d) => `'${d}'`).join(" ")})
provides=('agentscope')
conflicts=('agentscope')
source_x86_64=("${deb}::${download("${pkgver}", "AgentScope_${pkgver}_amd64.deb")}")
sha256sums_x86_64=('${h.deb}')

package() {
  bsdtar -xf data.tar.* -C "$pkgdir"
}
`;
}

export function srcinfo(version, h) {
  const deb = assetNames(version).deb;
  const lines = [
    `pkgbase = ${AUR.pkgname}`,
    `\tpkgdesc = ${DESCRIPTION}`,
    `\tpkgver = ${version}`,
    "\tpkgrel = 1",
    `\turl = ${HOMEPAGE}`,
    "\tarch = x86_64",
    ...AUR.licenses.map((l) => `\tlicense = ${l}`),
    ...AUR.depends.map((d) => `\tdepends = ${d}`),
    "\tprovides = agentscope",
    "\tconflicts = agentscope",
    `\tsource_x86_64 = ${deb}::${download(version, deb)}`,
    `\tsha256sums_x86_64 = ${h.deb}`,
    "",
    `pkgname = ${AUR.pkgname}`,
  ];
  return lines.join("\n") + "\n";
}

/** Every file to write, keyed by its path under the output dir. */
export function allFiles(version, sums) {
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error(`not a release version: ${version}`);
  const h = hashes(version, sums);
  const files = {
    "homebrew/Casks/agentscope.rb": cask(version, h),
    "scoop/bucket/agentscope.json": scoop(version, h),
    "aur/PKGBUILD": pkgbuild(version, h),
    "aur/.SRCINFO": srcinfo(version, h),
  };
  for (const [name, text] of Object.entries(winget(version, h))) {
    files[`winget/${name}`] = text;
  }
  return files;
}

function arg(name) {
  const i = process.argv.indexOf(`--${name}`);
  if (i < 0 || !process.argv[i + 1]) throw new Error(`missing --${name}`);
  return process.argv[i + 1];
}

function main() {
  const version = arg("version").replace(/^v/, "");
  const files = allFiles(version, parseSums(readFileSync(arg("sums"), "utf8")));
  const out = arg("out");
  for (const [path, text] of Object.entries(files)) {
    const full = join(out, path);
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, text);
    console.log(`wrote ${full}`);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
