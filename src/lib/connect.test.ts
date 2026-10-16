import assert from "node:assert/strict";
import test from "node:test";
import {
  commandDevice,
  emptyConnectState,
  nextRepeat,
  queueAction,
} from "./connect.ts";

test("controls target the active device and fall back to this computer only when idle", () => {
  const state = emptyConnectState();
  assert.equal(commandDevice(state), null);
  state.localDeviceId = "desktop";
  assert.equal(commandDevice(state), "desktop");
  state.activeDeviceId = "phone";
  assert.equal(commandDevice(state), "phone");
});
test("duplicate queue occurrences retain their own server keys", () => {
  const state = emptyConnectState();
  state.revision = "r1";
  state.queue = [
    {
      key: "0:first",
      uri: "same",
      name: "",
      artist: "",
      album: "",
      image: "",
      durationMs: 0,
    },
    {
      key: "1:second",
      uri: "same",
      name: "",
      artist: "",
      album: "",
      image: "",
      durationMs: 0,
    },
  ];
  assert.deepEqual(queueAction(state, "1:second", "r1", "remove"), {
    kind: "remove",
    key: "1:second",
    revision: "r1",
  });
  assert.deepEqual(queueAction(state, "0:first", "r1", "move", 1), {
    kind: "move",
    key: "0:first",
    revision: "r1",
    to: 1,
  });
});
test("a remote queue update invalidates an in-progress drag instead of moving a different occurrence", () => {
  const state = emptyConnectState();
  state.revision = "new";
  assert.throws(
    () => queueAction(state, "0:first", "old", "move", 1),
    /file a changé/,
  );
  assert.throws(
    () => queueAction(state, "missing", "new", "jump"),
    /file a changé/,
  );
});
test("queue move boundaries and repeat modes remain predictable", () => {
  const state = emptyConnectState();
  state.revision = "r";
  state.queue = [
    {
      key: "a",
      uri: "u",
      name: "",
      artist: "",
      album: "",
      image: "",
      durationMs: 0,
    },
  ];
  for (const to of [-1, 1, 0.5])
    assert.throws(() => queueAction(state, "a", "r", "move", to), /Position/);
  assert.equal(nextRepeat("off"), "all");
  assert.equal(nextRepeat("all"), "one");
  assert.equal(nextRepeat("one"), "off");
});
