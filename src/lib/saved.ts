/** Keeps late reads and an old account from overwriting a newer confirmed action. */
export class SavedTracks {
  private epoch = 0;
  private values = new Map<string, boolean>();
  private versions = new Map<string, number>();
  private pending = new Set<string>();

  get(uri: string): boolean | undefined {
    return this.values.get(uri);
  }
  busy(uri: string): boolean {
    return this.pending.has(uri);
  }
  snapshot(uris: string[]): { epoch: number; versions: number[] } {
    return {
      epoch: this.epoch,
      versions: uris.map((uri) => this.versions.get(uri) ?? 0),
    };
  }
  accept(
    uris: string[],
    values: boolean[],
    snapshot: ReturnType<SavedTracks["snapshot"]>,
  ): void {
    if (snapshot.epoch !== this.epoch || values.length !== uris.length) return;
    uris.forEach((uri, index) => {
      if (
        !this.pending.has(uri) &&
        (this.versions.get(uri) ?? 0) === snapshot.versions[index]
      )
        this.values.set(uri, values[index]);
    });
  }
  begin(uri: string): number | null {
    if (this.pending.has(uri)) return null;
    this.pending.add(uri);
    const version = (this.versions.get(uri) ?? 0) + 1;
    this.versions.set(uri, version);
    return this.epoch;
  }
  finish(uri: string, epoch: number, value?: boolean): void {
    if (epoch !== this.epoch) return;
    this.pending.delete(uri);
    this.versions.set(uri, (this.versions.get(uri) ?? 0) + 1);
    if (value !== undefined) this.values.set(uri, value);
  }
  clear(): void {
    this.epoch++;
    this.values.clear();
    this.versions.clear();
    this.pending.clear();
  }
}
