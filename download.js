// @ts-check

import { execSync } from "child_process";
import { chmodSync, existsSync, symlinkSync } from "fs";
import { exefile } from "./exe.js";
import https from "https";

async function main() {
  if (existsSync(exefile)) return;

  if (trySymlink()) return;

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

    console.log("Getting last successful build");
    /** @type {{build_num: number, workflows: {job_name: string}}[]} */
    const jobs = await fetchJson("/api/v1.1/project/gh/abihf/depgraph/tree/main?filter=successful&shallow=true");
    const job = jobs.find((j) => j.workflows.job_name == jobName);
    if (!job) throw new Error(`can not find last success build named "${jobName}"`);

    console.log(`Getting artifact from build ${job.build_num}`);
    /** @type {{path: string, url: string}[]} */
    const artifacts = await fetchJson(`/api/v1.1/project/gh/abihf/depgraph/${job.build_num}/artifacts`);
    const artifact = artifacts.find((a) => a.path === artifactName);
    if (!artifact) throw new Error(`can not find artifact named "${artifactName}" from build ${job.build_num}`);

    console.log(`Downloading ${artifact.url}`);
    exec(`wget -O "${exefile}" "${artifact.url}"`);
    chmodSync(exefile, '755');
  } catch (e) {
    console.error("Download error", e);
    console.log("Trying to build from source");
    exec("cargo build --release && ln -sf target/release/depgraph depgraph");
  }
}

main().catch((e) => {
  console.error("Error", e);
  process.exit(1);
});

function trySymlink() {
  for (const path of process.env.PATH?.split(":")) {
    const file = path + "/depgraph";
    if (existsSync(file)) {
      symlinkSync(file, exefile);
      return true;
    }
  }
  return false;
}

/**
 *
 * @param {string} path
 */
function fetchJson(path) {
  return new Promise((resolve) =>
    https
      .get(
        {
          hostname: "circleci.com",
          path,
          headers: {
            "circle-token": "bf4e4a555de7be26754a28669386fe34ce378a55",
            accept: "application/json",
          },
        },
        async (res) => {
          const chunks = [];
          for await (const chunk of res) {
            chunks.push(chunk);
          }
          resolve(JSON.parse(Buffer.concat(chunks).toString("utf-8")));
        }
      )
      .end()
  );
}

function exec(cmd) {
  return execSync(cmd, { stdio: "inherit" });
}
