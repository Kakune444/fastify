export type RepeatMode = "off" | "all" | "one";
export type QueueEntry<T> = {
  readonly id: number;
  track: T;
  /** Null identifies a manually queued occurrence, independent of its URI. */
  readonly index: number | null;
  anchor: number | null;
};

/** Occurrences, rather than track URIs, define playback order and history. */
export class PlaybackQueue<T extends { uri: string }> {
  private sequence = 0;
  private context: T[] = [];
  private items: QueueEntry<T>[] = [];
  private history: QueueEntry<T>[] = [];
  private visited = new Set<number>();
  private skipped = new Set<number>();
  current: QueueEntry<T> | undefined;
  name = "";
  shuffle = false;
  repeat: RepeatMode = "off";
  ended = false;

  private random: () => number;
  constructor(random: () => number = Math.random) {
    this.random = random;
  }

  get upcoming(): readonly QueueEntry<T>[] {
    return this.items;
  }
  get hasPrevious(): boolean {
    return this.history.length > 0;
  }
  get hasNext(): boolean {
    return (
      this.items.length > 0 ||
      (this.repeat === "all" && this.context.length > 0)
    );
  }

  private entry(track: T, index: number | null): QueueEntry<T> {
    return { id: ++this.sequence, track, index, anchor: index };
  }
  private mix(items: QueueEntry<T>[]): QueueEntry<T>[] {
    for (let i = items.length - 1; i > 0; i--) {
      const j = Math.floor(this.random() * (i + 1));
      [items[i], items[j]] = [items[j], items[i]];
    }
    return items;
  }
  private remember(): void {
    if (this.current) this.history.push(this.current);
    if (this.history.length > 200) this.history.shift();
  }
  private activate(entry: QueueEntry<T>): QueueEntry<T> {
    if (entry.index === null) entry.anchor = this.current?.anchor ?? null;
    this.remember();
    this.current = entry;
    if (entry.index !== null) this.visited.add(entry.index);
    this.ended = false;
    return entry;
  }

  reset(tracks: T[], index: number, name: string): QueueEntry<T> {
    if (!tracks[index]) throw new RangeError("Missing selected occurrence");
    this.context = [...tracks];
    this.history = [];
    this.visited = new Set([index]);
    this.skipped.clear();
    this.name = name;
    this.current = this.entry(tracks[index], index);
    this.items = tracks.flatMap((track, position) =>
      position !== index && (this.shuffle || position > index)
        ? [this.entry(track, position)]
        : [],
    );
    if (this.shuffle) this.mix(this.items);
    this.ended = false;
    return this.current;
  }
  clear(): void {
    this.context = [];
    this.items = [];
    this.history = [];
    this.visited.clear();
    this.skipped.clear();
    this.current = undefined;
    this.name = "";
    this.shuffle = false;
    this.repeat = "off";
    this.ended = false;
  }
  setShuffle(value: boolean): void {
    if (this.shuffle === value) return;
    this.shuffle = value;
    const manual = this.items.filter((entry) => entry.index === null);
    const existing = new Map(
      this.items
        .filter((entry) => entry.index !== null)
        .map((entry) => [entry.index, entry]),
    );
    const anchor = this.current?.anchor ?? -1;
    const context = this.context.flatMap((track, index) => {
      if (
        this.visited.has(index) ||
        this.skipped.has(index) ||
        (!value && index <= anchor)
      )
        return [];
      return [existing.get(index) ?? this.entry(track, index)];
    });
    this.items = [...manual, ...(value ? this.mix(context) : context)];
  }
  cycleRepeat(): void {
    this.repeat =
      this.repeat === "off" ? "all" : this.repeat === "all" ? "one" : "off";
  }
  enqueue(track: T): QueueEntry<T> {
    const entry = this.entry(track, null);
    let lastManual = -1;
    this.items.forEach((item, index) => {
      if (item.index === null) lastManual = index;
    });
    this.items.splice(lastManual + 1, 0, entry);
    return entry;
  }
  next(automatic = false): QueueEntry<T> | undefined {
    if (automatic && this.repeat === "one" && this.current) {
      this.ended = false;
      return this.current;
    }
    if (!this.items.length && this.repeat === "all" && this.context.length) {
      this.visited.clear();
      this.skipped.clear();
      this.items = this.context.map((track, index) => this.entry(track, index));
      if (this.shuffle) {
        this.mix(this.items);
        // Avoid a boundary repeat when another track exists in this context.
        if (this.items[0].track.uri === this.current?.track.uri) {
          const other = this.items.findIndex(
            (item) => item.track.uri !== this.current?.track.uri,
          );
          if (other > 0)
            [this.items[0], this.items[other]] = [
              this.items[other],
              this.items[0],
            ];
        }
      }
    }
    const entry = this.items.shift();
    if (!entry) {
      this.ended = automatic;
      return undefined;
    }
    return this.activate(entry);
  }
  previous(): QueueEntry<T> | undefined {
    const previous = this.history.pop();
    if (!previous) return undefined;
    if (this.current) this.items.unshift(this.current);
    this.current = previous;
    this.ended = false;
    return previous;
  }
  jump(id: number): QueueEntry<T> | undefined {
    const position = this.items.findIndex((entry) => entry.id === id);
    if (position < 0) return undefined;
    const passed = this.items.splice(0, position + 1);
    for (const entry of passed)
      if (entry.index !== null) this.visited.add(entry.index);
    return this.activate(passed[passed.length - 1]);
  }
  remove(id: number): void {
    const position = this.items.findIndex((entry) => entry.id === id);
    if (position < 0) return;
    const [entry] = this.items.splice(position, 1);
    if (entry.index !== null) this.skipped.add(entry.index);
  }
  move(id: number, to: number): void {
    const from = this.items.findIndex((entry) => entry.id === id);
    if (from < 0 || !Number.isInteger(to) || to < 0 || to >= this.items.length)
      return;
    const [entry] = this.items.splice(from, 1);
    this.items.splice(to, 0, entry);
  }
  updateTrack(uri: string, track: T): void {
    this.updateTracks(new Map([[uri, track]]));
  }
  updateTracks(tracks: ReadonlyMap<string, T>): void {
    this.context = this.context.map((item) => tracks.get(item.uri) ?? item);
    for (const entry of [this.current, ...this.items, ...this.history])
      if (entry) entry.track = tracks.get(entry.track.uri) ?? entry.track;
  }
}
