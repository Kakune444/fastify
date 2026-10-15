import { invoke } from "@tauri-apps/api/core";
import { collectCards, escapeHtml as esc, type Card } from "../lib/catalog.ts";

export type PlaylistEdit =
  | { kind: "details"; name: string; description: string }
  | { kind: "add"; uris: string[] }
  | { kind: "remove"; position: number; uri: string }
  | { kind: "move"; position: number; to: number; uri: string };
export type EditablePlaylist = {
  uri: string;
  name: string;
  description: string;
  revision: string;
};

type Hooks = {
  changed: (uri: string) => void;
  created: (card: Card) => void;
  error: (error: unknown) => void;
  pending: (value: boolean) => void;
};

/** Dialogs hold their own revision; later navigation cannot change a submitted edit. */
export class PlaylistDialogs {
  private dialog: HTMLDialogElement | null = null;
  private busy = false;
  constructor(private hooks: Hooks) {}

  close(): void {
    if (!this.busy) this.dialog?.close();
  }

  private open(title: string, content: string): HTMLDialogElement {
    this.dialog?.close();
    const dialog = document.createElement("dialog");
    dialog.className = "playlist-dialog";
    dialog.innerHTML = `<div class="dialog-heading"><h2 id="playlist-dialog-title">${esc(title)}</h2><button class="round dialog-close" aria-label="Fermer">×</button></div>${content}<p class="dialog-error" role="alert" hidden></p>`;
    dialog.setAttribute("aria-labelledby", "playlist-dialog-title");
    document.body.append(dialog);
    this.dialog = dialog;
    dialog
      .querySelector(".dialog-close")!
      .addEventListener("click", () => this.close());
    dialog.addEventListener("cancel", (event) => {
      if (this.busy) event.preventDefault();
    });
    dialog.addEventListener("close", () => {
      dialog.remove();
      if (this.dialog === dialog) this.dialog = null;
    });
    dialog.showModal();
    return dialog;
  }

  private error(dialog: HTMLDialogElement, error: unknown): void {
    const message =
      (error as { message?: string })?.message ||
      "La modification a échoué. Réessayez.";
    const output = dialog.querySelector<HTMLElement>(".dialog-error")!;
    output.textContent = message;
    output.hidden = false;
  }

  private setBusy(value: boolean): void {
    this.busy = value;
    this.hooks.pending(value);
    this.dialog?.setAttribute("aria-busy", String(value));
    this.dialog
      ?.querySelectorAll<
        HTMLButtonElement | HTMLInputElement | HTMLTextAreaElement
      >("button, input, textarea")
      .forEach((element) => {
        element.disabled = value;
      });
  }

  async change(
    uri: string,
    revision: string | null,
    edit: PlaylistEdit,
  ): Promise<boolean> {
    if (this.busy) return false;
    this.setBusy(true);
    try {
      await invoke("edit_playlist", { uri, revision, edit });
      this.hooks.changed(uri);
      return true;
    } catch (error) {
      if (this.dialog) this.error(this.dialog, error);
      else this.hooks.error(error);
      // Read the actual server state after a conflict or an unconfirmed response; never resend.
      const code = (error as { code?: string }).code;
      if (code === "CONFLICT" || code === "UNCERTAIN") this.hooks.changed(uri);
      return false;
    } finally {
      this.setBusy(false);
    }
  }

  details(playlist?: EditablePlaylist): void {
    if (this.busy) return;
    const dialog = this.open(
      playlist ? "Modifier les informations" : "Créer une playlist",
      `
      <form class="playlist-form">
        <label>Nom<input name="name" required maxlength="100" autocomplete="off" value="${esc(playlist?.name ?? "Ma playlist")}"/></label>
        <label>Description<textarea name="description" maxlength="300" rows="4" placeholder="Ajoutez une description (facultatif)">${esc(playlist?.description ?? "")}</textarea></label>
        <div class="dialog-actions"><button class="pill white" type="submit">${playlist ? "Enregistrer" : "Créer"}</button></div>
      </form>`,
    );
    const form = dialog.querySelector<HTMLFormElement>("form")!;
    const input = form.elements.namedItem("name") as HTMLInputElement;
    input.focus();
    input.select();
    form.addEventListener("submit", async (event) => {
      event.preventDefault();
      if (this.busy) return;
      const name = input.value.trim();
      const description = (
        form.elements.namedItem("description") as HTMLTextAreaElement
      ).value;
      if (!name) {
        input.focus();
        return;
      }
      if (playlist) {
        if (
          await this.change(playlist.uri, playlist.revision, {
            kind: "details",
            name,
            description,
          })
        )
          dialog.close();
        return;
      }
      this.setBusy(true);
      try {
        const uri = await invoke<string>("create_playlist", {
          name,
          description,
        });
        dialog.close();
        this.hooks.created({
          uri,
          name,
          subtitle: "Playlist",
          image: "",
          kind: "playlist",
          durationMs: 0,
        });
      } catch (error) {
        this.error(dialog, error);
      } finally {
        this.setBusy(false);
      }
    });
  }

  async add(trackUri: string): Promise<void> {
    if (this.busy || !trackUri.startsWith("spotify:track:")) return;
    const dialog = this.open(
      "Ajouter à une playlist",
      '<label class="playlist-search">Rechercher une playlist<input type="search" placeholder="Rechercher" autocomplete="off"/></label><div class="playlist-options" role="status">Chargement des playlists…</div>',
    );
    const list = dialog.querySelector<HTMLElement>(".playlist-options")!;
    const input = dialog.querySelector<HTMLInputElement>("input")!;
    input.focus();
    try {
      const result = await invoke<{ items: unknown[] }>(
        "get_writable_playlists",
      );
      if (!dialog.open) return;
      const playlists = collectCards(result.items, 20_000);
      list.removeAttribute("role");
      const render = () => {
        const query = input.value.trim().toLocaleLowerCase();
        const matches = playlists.filter((card) =>
          card.name.toLocaleLowerCase().includes(query),
        );
        list.innerHTML =
          matches
            .map(
              (card) =>
                `<button class="playlist-option" data-target="${esc(card.uri)}"><span class="playlist-option-art">♫</span><span><strong>${esc(card.name)}</strong><small>${esc(card.subtitle)}</small></span></button>`,
            )
            .join("") ||
          '<p class="playlist-no-results">Aucune playlist disponible.</p>';
      };
      input.addEventListener("input", render);
      render();
      list.addEventListener("click", async (event) => {
        const button = (event.target as HTMLElement).closest<HTMLButtonElement>(
          "[data-target]",
        );
        if (
          button &&
          (await this.change(button.dataset.target!, null, {
            kind: "add",
            uris: [trackUri],
          }))
        )
          dialog.close();
      });
    } catch (error) {
      if (dialog.open) {
        list.textContent = "Impossible de charger les playlists.";
        this.error(dialog, error);
      }
    }
  }
}
