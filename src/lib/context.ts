export type ContextPage = {
  uris: string[];
  next: string | null;
  revision: string | null;
};

/** Fetch identifiers only. The next request is never sent for an abandoned selection. */
export async function readContext(
  read: (cursor: string, revision: string | null) => Promise<ContextPage>,
  cancelled: () => boolean,
  revision: string | null,
): Promise<string[] | null> {
  const uris: string[] = [];
  const seen = new Set<string>();
  let cursor = "";
  for (let page = 0; page < 100; page++) {
    if (cancelled()) return null;
    if (seen.has(cursor))
      throw {
        code: "API_CHANGED",
        message: "Spotify a renvoyé une page de lecture identique.",
      };
    seen.add(cursor);
    const result = await read(cursor, revision);
    if (cancelled()) return null;
    if (revision !== null && result.revision !== revision)
      throw {
        code: "CONFLICT",
        message: "La playlist a changé. Rechargez-la avant de la lire.",
      };
    revision ??= result.revision;
    uris.push(...result.uris);
    if (uris.length > 100_000) break;
    if (result.next === null) return uris;
    cursor = result.next;
  }
  throw {
    code: "CONTEXT_TOO_LARGE",
    message:
      "Cette sélection est trop volumineuse pour être préparée entièrement.",
  };
}
