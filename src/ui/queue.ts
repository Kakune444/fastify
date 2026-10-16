import { invoke } from "@tauri-apps/api/core";
import { escapeHtml as esc, formatTime, type Card } from "../lib/catalog.ts";
import { PlaybackQueue, type QueueEntry } from "../lib/queue.ts";

type Hooks = {
  active: () => boolean;
  cover: (card: Card) => string;
  save: (uri: string) => string;
  menu: (uri: string, position?: number, queueId?: number) => string;
  queued: (id: number) => boolean;
  rendered: (uris: string[]) => void;
  error: (error: unknown) => void;
};
export class QueueView {
  private version = 0;
  private pending = new Set<string>();
  private visible = 40;
  private queue: PlaybackQueue<Card>;
  private hooks: Hooks;
  constructor(queue: PlaybackQueue<Card>, hooks: Hooks) {
    this.queue = queue;
    this.hooks = hooks;
  }
  reset(): void {
    this.version++;
    this.pending.clear();
    this.visible = 40;
  }
  more(): void {
    this.visible += 40;
    this.render();
  }
  private row(entry: QueueEntry<Card>, index: number, current = false): string {
    const card = entry.track;
    return `<div class="queue-row" ${current ? "" : `draggable="true" tabindex="0" data-queue-id="${entry.id}" data-queue-position="${index}" title="Glissez pour déplacer · Alt + ↑ ou ↓"`}>
      <button class="queue-play" ${current ? 'data-queue-current="true"' : `data-queue-play="${entry.id}"`}>${this.hooks.cover(card)}<span><strong>${esc(card.name)}</strong><small>${esc(card.subtitle)}</small></span></button>
      ${this.hooks.save(card.uri)}<span>${card.durationMs ? formatTime(card.durationMs) : ""}</span>${this.hooks.menu(card.uri, undefined, current ? undefined : entry.id)}</div>`;
  }
  render(): void {
    if (!this.hooks.active()) return;
    const current = this.queue.current;
    const upcoming = this.queue.upcoming.slice(0, this.visible);
    const rows = upcoming
      .map((entry, index) => {
        const queued = this.hooks.queued(entry.id);
        const heading =
          index === 0 || queued !== this.hooks.queued(upcoming[index - 1].id)
            ? `<h2>${queued ? "À suivre dans la file d’attente" : `À suivre${this.queue.name ? ` · ${esc(this.queue.name)}` : ""}`}</h2>`
            : "";
        return heading + this.row(entry, index);
      })
      .join("");
    document.getElementById("main-content")!.innerHTML =
      `<div class="queue-page"><h1>File d’attente</h1>
      ${current ? `<h2>En cours de lecture</h2>${this.row(current, 0, true)}` : ""}
      ${upcoming.length ? rows : '<h2>À suivre</h2><p class="queue-empty">Aucun titre dans la file d’attente.</p>'}
      ${this.queue.upcoming.length > upcoming.length ? '<button class="pill" id="queue-more">Afficher plus</button>' : ""}</div>`;
    const uris = [
      ...new Set([
        ...upcoming.map((entry) => entry.track.uri),
        ...(current ? [current.track.uri] : []),
      ]),
    ];
    this.hooks.rendered(uris);
    const missing = upcoming
      .map((entry) => entry.track)
      .filter(
        (card) =>
          card.name === "Chargement du titre…" &&
          /^spotify:track:[A-Za-z0-9]{22}$/.test(card.uri) &&
          !this.pending.has(card.uri),
      );
    const batch = [...new Set(missing.map((card) => card.uri))].slice(0, 40);
    if (!batch.length) return;
    batch.forEach((uri) => this.pending.add(uri));
    const version = this.version;
    void invoke<
      {
        uri: string;
        name: string;
        artist: string;
        image: string;
        durationMs: number;
      }[]
    >("get_track_details", { uris: batch })
      .then((tracks) => {
        if (version !== this.version) return;
        this.queue.updateTracks(
          new Map(
            tracks.map((track) => [
              track.uri,
              {
                uri: track.uri,
                name: track.name,
                subtitle: track.artist,
                image: track.image,
                kind: "track",
                durationMs: track.durationMs,
              },
            ]),
          ),
        );
        this.render();
      })
      .catch((error) => {
        if (version !== this.version) return;
        batch.forEach((uri) => this.pending.delete(uri));
        if (this.hooks.active()) this.hooks.error(error);
      });
  }
}
