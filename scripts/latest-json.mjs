// Writes latest.json, the file every installed Glowtune checks for updates
// (see app/src-tauri/src/update.rs). The Release workflow runs it after the
// signed build:
//
//   node scripts/latest-json.mjs <version> <installer.exe.sig> <installer URL> [notes.md] > latest.json
//
// The notes shown in the update pop-up are the release notes' "What's new"
// and "Fixes" sections (the whole file if it has neither).
//
// It first checks the signature against the public key built into the app
// (tauri.conf.json), so a wrong key in the repo secrets fails the release
// here instead of every installed Glowtune refusing the update.
import { readFileSync, existsSync } from "node:fs";
import { createHash, createPublicKey, verify } from "node:crypto";

const [version, sigFile, url, notesFile] = process.argv.slice(2);
if (!version || !sigFile || !url) {
  console.error("usage: latest-json.mjs <version> <sig file> <installer url> [notes.md]");
  process.exit(2);
}
if (!/^\d+\.\d+\.\d+([-+][\w.]+)?$/.test(version)) {
  console.error(`not a version: ${version}`);
  process.exit(2);
}
if (!url.startsWith("https://")) {
  console.error(`the installer URL must be https: ${url}`);
  process.exit(2);
}
const signature = readFileSync(sigFile, "utf8").trim();
if (!signature) {
  console.error(`${sigFile} is empty`);
  process.exit(2);
}
const pubkey =
  process.env.UPDATER_PUBKEY ??
  JSON.parse(readFileSync(new URL("../app/src-tauri/tauri.conf.json", import.meta.url), "utf8")).plugins.updater.pubkey;
const problem = checkSignature(readFileSync(sigFile.replace(/\.sig$/, "")), signature, pubkey);
if (problem) {
  console.error(`signature check failed: ${problem}`);
  process.exit(1);
}

/**
 * Checks a Tauri (minisign) signature of `file` against the base64 public
 * key from tauri.conf.json. Returns null when it's good, or the reason.
 */
export function checkSignature(file, signatureB64, pubkeyB64) {
  // Both are base64 of minisign's text files; the second line is the payload.
  const line = (b64) => Buffer.from(b64, "base64").toString("utf8").split(/\r?\n/)[1] ?? "";
  const key = Buffer.from(line(pubkeyB64), "base64"); // "Ed" + key id (8) + key (32)
  const sig = Buffer.from(line(signatureB64), "base64"); // "ED" + key id (8) + signature (64)
  if (key.length !== 42) return "the app's public key isn't a minisign key";
  if (sig.length !== 74) return "the .sig file isn't a minisign signature";
  const id = (b) => Buffer.from(b.subarray(2, 10)).reverse().toString("hex").toUpperCase();
  if (id(key) !== id(sig)) return `signed with key ${id(sig)}, but the app trusts key ${id(key)} (wrong TAURI_SIGNING_PRIVATE_KEY secret?)`;
  const alg = sig.subarray(0, 2).toString();
  const message = alg === "ED" ? createHash("blake2b512").update(file).digest() : file;
  const spki = Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), key.subarray(10)]);
  const ok = verify(null, message, createPublicKey({ key: spki, format: "der", type: "spki" }), sig.subarray(10));
  return ok ? null : "the signature doesn't match the installer";
}

export function updateNotes(md) {
  const lines = md.split(/\r?\n/);
  const keep = /^##\s+(what'?s new|fixes)\b/i;
  const out = [];
  let on = false;
  for (const line of lines) {
    if (/^##\s/.test(line)) on = keep.test(line);
    if (on) out.push(line);
  }
  const picked = out.join("\n").trim();
  return picked || lines.filter((l) => !/^#\s/.test(l)).join("\n").trim();
}

const notes = notesFile && existsSync(notesFile) ? updateNotes(readFileSync(notesFile, "utf8")) : "";
// Only installs made by the NSIS installer look for "windows-x86_64-nsis";
// there is deliberately no bare "windows-x86_64", so a plain exe (the Store
// copy, test builds) never finds an installer to run.
const platform = { signature, url };
const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
  platforms: { "windows-x86_64-nsis": platform },
};
process.stdout.write(JSON.stringify(manifest, null, 2) + "\n");
