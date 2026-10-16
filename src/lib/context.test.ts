import test from "node:test";
import assert from "node:assert/strict";
import { readContext } from "./context.ts";

test("context pagination keeps duplicate occurrences and pins the first server revision", async () => {
  const calls: [string, string | null][] = [];
  const all = await readContext(
    async (cursor, revision) => {
      calls.push([cursor, revision]);
      return cursor === ""
        ? { uris: ["a", "a"], next: "2", revision: "v1" }
        : { uris: ["b"], next: null, revision: "v1" };
    },
    () => false,
    null,
  );
  assert.deepEqual(all, ["a", "a", "b"]);
  assert.deepEqual(calls, [
    ["", null],
    ["2", "v1"],
  ]);
});
test("a playlist revision change cannot mix snapshots", async () => {
  await assert.rejects(
    readContext(
      async () => ({ uris: ["a"], next: null, revision: "new" }),
      () => false,
      "old",
    ),
    { code: "CONFLICT" },
  );
});
test("abandoning a selection stops pagination after the in-flight read", async () => {
  let cancelled = false;
  let calls = 0;
  const result = await readContext(
    async () => {
      calls++;
      cancelled = true;
      return { uris: ["a"], next: "next", revision: null };
    },
    () => cancelled,
    null,
  );
  assert.equal(result, null);
  assert.equal(calls, 1);
});
test("a repeated cursor is rejected instead of looping indefinitely", async () => {
  await assert.rejects(
    readContext(
      async () => ({ uris: [], next: "", revision: null }),
      () => false,
      null,
    ),
    { code: "API_CHANGED" },
  );
});
