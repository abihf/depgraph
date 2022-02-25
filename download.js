// @ts-check

import { execSync } from "child_process";
import { chmodSync, existsSync, readFileSync, rmSync, symlinkSync } from "fs";
import { exefile } from "./exe.js";

async function main() {
  const { version } = JSON.parse(readFileSync("package.json", "utf-8"));

  if (existsSync(exefile)) {
    if (checkVersion(version, exefile)) return;
    rmSync(exefile, { force: true });
  }

  if (trySymlink(version)) return;

  try {
    /** @type {string} */
    const jobName = {
      linux: "build-linux",
      darwin: "build-mac",
    }[process.platform];
    if (!jobName) throw new Error(`Can not find build for platform ${process.platform}`);

    /** @type {string} */
    const artifactName = {
      "linux-x64": "depgraph-x86_64-unknown-linux-gnu",
      "darwin-x64": "depgraph-x86_64-apple-darwin",
      "darwin-arm64": "depgraph-aarch64-apple-darwin",
    }[`${process.platform}-${process.arch}`];
    if (!artifactName) throw new Error(`Can not find build for platform ${process.platform} arch ${process.arch}`);

    const url = `https://github.com/abihf/depgraph/releases/download/v${version}/${artifactName}`;
    console.log(`Downloading ${url}`);
    exec(`wget -O "${exefile}" "${url}"`);
    chmodSync(exefile, "755");
  } catch (e) {
    console.error("Download error", e);
    console.log("Trying to build from source");
    exec(`sed -i.bak '/^version = /c\\version = \"${version}\"' Cargo.toml`);
    exec("cargo build --release && ln -sf target/release/depgraph depgraph");
  }
}

if (!process.env.DEPGRAPH_SKIP_DOWNLOAD) {
  main().catch((e) => {
    console.error("Error", e);
    process.exit(1);
  });
}

/**
 *
 * @param {string} version
 * @param {string} file
 */
function checkVersion(version, file) {
  try {
    const res = execSync(file + " --version", { stdio: ["ignore", "pipe", "inherit"] })
      .toString("utf-8")
      .trim();
    return res === version;
  } catch (_e) {
    return false;
  }
}

/**
 *
 * @param {string} version
 */
function trySymlink(version) {
  for (const path of process.env.PATH?.split(":")) {
    const file = path + "/depgraph";
    if (existsSync(file) && checkVersion(version, file)) {
      symlinkSync(file, exefile);
      return true;
    }
  }
  return false;
}

/**
 *
 * @param {string} cmd
 */
function exec(cmd) {
  return execSync(cmd, { stdio: "inherit" });
}
