import { test } from "node:test";
import assert from "node:assert/strict";
import { SavedTracks } from "./saved.ts";

test("a late status read cannot undo a confirmed favorite change", () => {
  const saved = new SavedTracks();
  const read = saved.snapshot(["track"]);
  const operation = saved.begin("track")!;
  saved.finish("track", operation, true);
  saved.accept(["track"], [false], read);
  assert.equal(saved.get("track"), true);
});
test("an unsuccessful write retains the prior state and blocks duplicate submissions", () => {
  const saved = new SavedTracks();
  saved.accept(["track"], [false], saved.snapshot(["track"]));
  const operation = saved.begin("track")!;
  assert.equal(saved.begin("track"), null);
  saved.finish("track", operation);
  assert.equal(saved.get("track"), false);
  assert.equal(saved.busy("track"), false);
});
test("logout invalidates pending reads and writes from the previous account", () => {
  const saved = new SavedTracks();
  const read = saved.snapshot(["track"]);
  const operation = saved.begin("track")!;
  saved.clear();
  saved.finish("track", operation, true);
  saved.accept(["track"], [true], read);
  assert.equal(saved.get("track"), undefined);
  assert.equal(saved.busy("track"), false);
});
