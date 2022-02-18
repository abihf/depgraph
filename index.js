// @ts-check
import { spawn } from "child_process";
import { once } from "events";
import { createInterface } from "readline";
import { exefile } from "./exe.js";

/**
 * @param {Iterable<string> | AsyncIterable<string>} files
 */
export async function* analyze(files) {
  const proc = spawn(exefile, {
    stdio: ["pipe", "pipe", "inherit"],
  });

  const inputPromise = (async () => {
    for await (const file of toAsyncIterable(files)) {
      if (!proc.stdin.write(file + "\n", "utf-8")) {
        await once(proc.stdin, "drain");
      }
    }
    proc.stdin.end();
  })();

  for await (const line of createInterface(proc.stdout)) {
    /** @type {import("./type").Item} */
    const parsed = JSON.parse(line);
    yield parsed;
  }
  await once(proc, "close");
  if (proc.exitCode !== 0) throw new Error(`Process error with code ${proc.exitCode}`);
  await inputPromise;
}

/**
 * @template T
 * @param {Iterable<T> | AsyncIterable<T>} items
 */
async function* toAsyncIterable(items) {
  yield* items;
}
