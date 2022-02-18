import { Item } from "./type";

export function analyze(files: Iterable<string> | AsyncIterable<string>): AsyncGenerator<Item, void>
