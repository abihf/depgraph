// @ts-check
import { spawn } from "child_process";
import { once } from "events";
import { createInterface } from "readline";

const exefile = new URL("./depgraph", import.meta.url).pathname;

/**
 *
 * @param {Iterable<string> | AsyncIterable<string>} files
 */
export async function* analyze(files) {
  const proc = spawn(exefile, {
    stdio: ["pipe", "pipe", "inherit"],
  });
  if (Symbol.asyncIterator in files) {
    for await (const file of files) {
      if (!proc.stdin.write(file + "\n", "utf-8")) {
        await once(proc.stdin, "drain");
      }
    }
  } else {
    // @ts-expect-error
    for (const file of files) {
      if (!proc.stdin.write(file + "\n", "utf-8")) {
        await once(proc.stdin, "drain");
      }
    }
  }
  proc.stdin.end();

  for await (const line of createInterface(proc.stdout)) {
    /** @type {import("./type").Item} */
    const parsed = JSON.parse(line);
    yield parsed;
  }
  await once(proc, "close");
  if (proc.exitCode !== 0) throw new Error(`Process error with code ${proc.exitCode}`);
}
