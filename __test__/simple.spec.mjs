// @ts-check

import test from "ava";
import { analyze } from "../index.js";

test("reexport only file", async (t) => {
  const source = `
  export * from 'module_a';
  export * as b from './module_b'; 
  export {b as c, d as e, default as ModuleC} from './dir/module_c'
  `;
  const res = await analyze("reexport.js", source);

  t.is(res.length, 3);
  t.like(res[0], { name: "module_a", exports: [["*", "*"]] });
  t.like(res[1], { name: "./module_b", exports: [["b", "*"]] });
  t.like(res[2], {
    name: "./dir/module_c",
    exports: [
      ["c", "b"],
      ["e", "d"],
      ["ModuleC", "default"],
    ],
  });
});
