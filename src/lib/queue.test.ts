import test from "node:test";
import assert from "node:assert/strict";
import { PlaybackQueue } from "./queue.ts";

const tracks = (...uris: string[]) => uris.map((uri) => ({ uri }));
const uris = (queue: PlaybackQueue<{ uri: string }>) =>
  queue.upcoming.map((entry) => entry.track.uri);

test("duplicate URIs remain separate occurrences through next, previous and removal", () => {
  const queue = new PlaybackQueue<{ uri: string }>();
  const first = queue.reset(tracks("a", "a", "b", "a"), 0, "Playlist");
  const second = queue.next()!;
  assert.notEqual(first.id, second.id);
  assert.equal(second.track.uri, "a");
  queue.remove(queue.upcoming[1].id);
  assert.deepEqual(uris(queue), ["b"]);
  assert.equal(queue.previous()?.id, first.id);
  assert.equal(queue.next()?.id, second.id);
  assert.equal(queue.next()?.track.uri, "b");
  assert.equal(queue.next(true), undefined);
  assert.equal(queue.ended, true);
});

test("shuffle includes the full context, retains the current occurrence and visits each once", () => {
  const queue = new PlaybackQueue<{ uri: string }>(() => 0.2);
  const source = Array.from({ length: 95 }, (_, index) => ({
    uri: String(index),
  }));
  const current = queue.reset(source, 55, "Album");
  queue.setShuffle(true);
  assert.equal(queue.current?.id, current.id);
  assert.equal(queue.upcoming.length, 94);
  const visited = [current.track.uri];
  while (queue.hasNext) visited.push(queue.next()!.track.uri);
  assert.equal(new Set(visited).size, 95);
  assert.notDeepEqual(
    visited.slice(1),
    source.filter((track) => track !== source[55]).map((track) => track.uri),
  );
});

test("repeat one responds to natural end while manual next advances", () => {
  const queue = new PlaybackQueue<{ uri: string }>();
  const selected = queue.reset(tracks("a", "b"), 0, "Album");
  queue.repeat = "one";
  assert.equal(queue.next(true)?.id, selected.id);
  assert.equal(queue.upcoming.length, 1);
  assert.equal(queue.next(false)?.track.uri, "b");
  assert.equal(queue.hasNext, true);
  assert.equal(queue.next(false)?.track.uri, "a");
});

test("repeat all starts a fresh context cycle and supports a single-track context", () => {
  const queue = new PlaybackQueue<{ uri: string }>();
  queue.reset(tracks("a", "b"), 1, "Album");
  queue.repeat = "all";
  assert.equal(queue.next(true)?.track.uri, "a");
  assert.equal(queue.next(true)?.track.uri, "b");
  const one = queue.reset(tracks("solo"), 0, "Single");
  assert.equal(queue.hasNext, true);
  const repeated = queue.next(true)!;
  assert.equal(repeated.track.uri, "solo");
  assert.notEqual(repeated.id, one.id);
});

test("manual additions keep FIFO priority and previous follows the actual listening history", () => {
  const queue = new PlaybackQueue<{ uri: string }>();
  queue.reset(tracks("a", "b", "c"), 0, "Album");
  queue.enqueue({ uri: "manual-1" });
  queue.enqueue({ uri: "manual-2" });
  assert.deepEqual(uris(queue), ["manual-1", "manual-2", "b", "c"]);
  queue.next();
  queue.next();
  queue.next();
  assert.equal(queue.previous()?.track.uri, "manual-2");
  assert.equal(queue.next()?.track.uri, "b");
  assert.equal(queue.next()?.track.uri, "c");
});

test("moving and jumping target occurrence IDs without changing the source playlist", () => {
  const queue = new PlaybackQueue<{ uri: string }>();
  const source = tracks("a", "b", "a", "c");
  queue.reset(source, 0, "Playlist");
  const duplicate = queue.upcoming[1];
  queue.move(duplicate.id, 0);
  assert.deepEqual(uris(queue), ["a", "b", "c"]);
  const last = queue.upcoming[2];
  assert.equal(queue.jump(last.id)?.track.uri, "c");
  assert.equal(queue.previous()?.track.uri, "a");
  assert.deepEqual(source, tracks("a", "b", "a", "c"));
});

test("switching shuffle off restores order and keeps explicitly removed tracks skipped", () => {
  const queue = new PlaybackQueue<{ uri: string }>(() => 0);
  queue.reset(tracks("a", "b", "c", "d"), 0, "Album");
  queue.remove(queue.upcoming[1].id);
  queue.enqueue({ uri: "manual" });
  queue.setShuffle(true);
  queue.setShuffle(false);
  assert.deepEqual(uris(queue), ["manual", "b", "d"]);
});

test("a queued track can start playback without an existing context; logout clears all modes", () => {
  const queue = new PlaybackQueue<{ uri: string }>();
  queue.enqueue({ uri: "queued" });
  assert.equal(queue.hasNext, true);
  assert.equal(queue.next()?.track.uri, "queued");
  queue.setShuffle(true);
  queue.cycleRepeat();
  queue.clear();
  assert.equal(queue.current, undefined);
  assert.equal(queue.hasNext, false);
  assert.equal(queue.shuffle, false);
  assert.equal(queue.repeat, "off");
});
