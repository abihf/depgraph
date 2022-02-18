import { fetch } from "fetch-h2";
import { execSync } from "node:child_process";
import { createWriteStream, chmodSync, existsSync } from "node:fs";
import { pipeline } from "node:stream";
import { promisify } from "node:util";

async function main() {
  if (existsSync("depgraph")) return;

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
    const jobs = await fetchJson(
      "https://circleci.com/api/v1.1/project/gh/abihf/depgraph/tree/main?filter=successful&shallow=true"
    );
    const job = jobs.find((j) => j.workflows.job_name == jobName);
    if (!job) throw new Error(`can not find last success build named "${jobName}"`);

    console.log(`Getting artifact from build ${job.build_num}`);
    /** @type {{path: string, url: string}[]} */
    const artifacts = await fetchJson(
      `https://circleci.com/api/v1.1/project/gh/abihf/depgraph/${job.build_num}/artifacts`
    );
    const artifact = artifacts.find((a) => a.path === artifactName);
    if (!artifact) throw new Error(`can not find artifact named "${artifactName}" from build ${job.build_num}`);

    console.log(`Downloading ${artifact.url}`);
    const res = await fetch(artifact.url, { redirect: "follow" });
    const body = await res.readable();
    await promisify(pipeline)(body, createWriteStream("depgraph"));
    chmodSync("depgraph", 0x755);
  } catch (e) {
    console.error("Download error", e);
    console.log("Trying to build from source");
    execSync("cargo build --release && ln -sf target/release/depgraph depgraph");
  }
}

main().catch((e) => {
  console.error("Error", e);
  process.exit(1);
});

async function fetchJson(url) {
  const res = await fetch(url, {
    headers: {
      "circle-token": "bf4e4a555de7be26754a28669386fe34ce378a55",
      accept: "application/json",
    },
  });
  return res.json();
}
