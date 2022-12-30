// @ts-check

import { execSync } from "child_process";
import { once } from "events";
import { createWriteStream, existsSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "fs";
import { chmod } from "fs/promises";
import http from "http";
import https from "https";
import { URL } from "url";
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
    const artifactName = {
      "linux-x64": "depgraph-x86_64-unknown-linux-gnu",
      "linux-arm64": "depgraph-aarch64-unknown-linux-gnu",
      "darwin-x64": "depgraph-x86_64-apple-darwin",
      "darwin-arm64": "depgraph-aarch64-apple-darwin",
    }[`${process.platform}-${process.arch}`];
    if (!artifactName) throw new Error(`Can not find build for platform ${process.platform} arch ${process.arch}`);

    const url = `https://github.com/abihf/depgraph/releases/download/v${version}/${artifactName}`;
    console.log(`Downloading ${url}`);
    await downloadExe(url);
  } catch (e) {
    console.error("Download error", e);
    console.log("Trying to build from source");
    replaceCargoVersion(version);
    exec("cargo build --release && ln -sf target/release/depgraph depgraph");
    console.log("Build successful");
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
 * @param {string} urlStr
 */
async function downloadExe(urlStr) {
  let url = new URL(urlStr);
  let maxRedirect = 5;

  while (maxRedirect > 0) {
    const res = await new Promise((resolve, reject) =>
      (url.protocol === "https:" ? https : http).get(url, resolve).on("error", reject)
    );
    const status = res.statusCode;
    if (status === 200) {
      const file = createWriteStream(exefile);
      const pipe = res.pipe(file);
      await once(pipe, "close");
      await chmod(exefile, "755");
      return;
    } else if ([301, 302, 303, 307, 308].includes(status)) {
      url = new URL(res.headers.location, url);
      maxRedirect--;
    } else {
      throw new Error(`unexecpected code ${status} from ${url.toString()}`);
    }
  }
  throw new Error(`reach max redirect for ${url.toString()}`);
}

/**
 *
 * @param {string} version
 */
function replaceCargoVersion(version) {
  let content = readFileSync("Cargo.toml", "utf-8");
  content = content.replace(/^version = .*$/, `version = "${version}"`);
  writeFileSync("Cargo.toml", content, "utf-8");
}

/**
 *
 * @param {string} cmd
 */
 function exec(cmd) {
  return execSync(cmd, { stdio: ["ignore", "inherit", "inherit"] });
}
