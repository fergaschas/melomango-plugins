// Builds a plugin for the current platform, publishes it as a GitHub release asset
// and records its download URL + SHA-256 in index.json.
//
// Usage: node scripts/release.mjs <plugin-folder>      e.g. node scripts/release.mjs youtube
// Needs: cargo, gh (logged in). Run it once per OS to add more platforms to the same version.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const folder = process.argv[2];
if (!folder) {
  console.error("Uso: node scripts/release.mjs <carpeta-del-plugin>");
  process.exit(1);
}

const sh = (cmd, args, opts = {}) => execFileSync(cmd, args, { encoding: "utf8", ...opts });

const pluginDir = path.join(root, folder);
const manifest = JSON.parse(fs.readFileSync(path.join(pluginDir, "plugin.json"), "utf8"));
const { id, name, version } = manifest;

const os = { win32: "windows", darwin: "macos", linux: "linux" }[process.platform] ?? process.platform;
const arch = { x64: "x86_64", arm64: "aarch64" }[process.arch] ?? process.arch;
const platform = `${os}-${arch}`;
const exe = manifest.executable + (process.platform === "win32" ? ".exe" : "");

console.log(`==> Compilando ${name} ${version} para ${platform}`);
sh("cargo", ["build", "--release", "--manifest-path", path.join(pluginDir, "Cargo.toml")], { stdio: "inherit" });

// Stage plugin.json + executable and zip them (files at the zip root).
const dist = path.join(root, "dist");
const stage = path.join(dist, id);
fs.rmSync(stage, { recursive: true, force: true });
fs.mkdirSync(stage, { recursive: true });
fs.copyFileSync(path.join(pluginDir, "plugin.json"), path.join(stage, "plugin.json"));
fs.copyFileSync(path.join(pluginDir, "target", "release", exe), path.join(stage, exe));

const zipName = `${id}-${version}-${platform}.zip`;
const zipPath = path.join(dist, zipName);
fs.rmSync(zipPath, { force: true });
if (process.platform === "linux") {
  sh("zip", ["-j", zipPath, path.join(stage, "plugin.json"), path.join(stage, exe)]);
} else {
  // bsdtar (Windows 10+ and macOS) writes zip files with -a.
  const tar = process.platform === "win32" ? path.join(process.env.SystemRoot, "System32", "tar.exe") : "tar";
  sh(tar, ["-a", "-c", "-f", zipPath, "-C", stage, "plugin.json", exe]);
}
const sha256 = createHash("sha256").update(fs.readFileSync(zipPath)).digest("hex");

// Publish to the GitHub release <id>-v<version>.
const repo = sh("gh", ["repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"], { cwd: root }).trim();
const tag = `${id}-v${version}`;
let exists = true;
try {
  sh("gh", ["release", "view", tag, "--repo", repo], { stdio: "ignore" });
} catch {
  exists = false;
}
if (!exists) {
  console.log(`==> Creando release ${tag}`);
  sh("gh", ["release", "create", tag, "--repo", repo, "--title", `${name} ${version}`, "--notes",
    `${name} ${version} para Melomango. Instálalo desde la app: Plugins → Explorar.`]);
}
console.log(`==> Subiendo ${zipName}`);
sh("gh", ["release", "upload", tag, zipPath, "--repo", repo, "--clobber"], { stdio: "inherit" });

// Update index.json.
const indexPath = path.join(root, "index.json");
const index = JSON.parse(fs.readFileSync(indexPath, "utf8"));
let entry = index.plugins.find((p) => p.id === id);
if (!entry) {
  entry = { id };
  index.plugins.push(entry);
}
const sameVersion = entry.version === version;
Object.assign(entry, {
  name,
  version,
  description: manifest.description ?? null,
  author: manifest.author ?? null,
  homepage: manifest.homepage ?? null,
  tags: manifest.tags ?? [],
  downloads: sameVersion ? entry.downloads ?? {} : {},
});
entry.downloads[platform] = {
  url: `https://github.com/${repo}/releases/download/${tag}/${zipName}`,
  sha256,
};
index.plugins.sort((a, b) => a.name.localeCompare(b.name));
fs.writeFileSync(indexPath, JSON.stringify(index, null, 2) + "\n");

console.log(`\n${name} ${version} publicado para ${platform}.`);
console.log("Ahora haz commit y push de index.json para que la app lo vea.");
