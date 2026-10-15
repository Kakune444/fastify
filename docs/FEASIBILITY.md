# Étude de faisabilité

Date : 15 octobre 2026. Les conclusions ci-dessous distinguent ce qui est observé
localement de ce qui est documenté et de ce qui reste à tester.

## Observations locales

- Fiddler Classic est ouvert et capture déjà du trafic HTTPS déchiffré de Spotify.
- `gew1-spclient.spotify.com/extended-metadata/v0/extended-metadata` répond en
  HTTP 200 avec du `application/protobuf` (plusieurs réponses autour de 16 Ko).
- `spclient.wg.spotify.com/gabo-receiver-service/public/v3/events/` reçoit des
  événements et répond en HTTP 200. Il ne constitue pas une API de bibliothèque.
- Les sessions `CONNECT` ne sont que des tunnels et ne décrivent pas les appels.
- Aucun schéma Protobuf ni mécanisme de lecture autonome n'est encore validé.
- Un essai ponctuel de `/v1/me`, `/v1/me/playlists` et `/v1/search` avec la session
  capturée a reçu HTTP 429 (`API rate limit exceeded`). Aucun nouvel essai répété
  n'est lancé sur ces routes ; cette session ne valide donc pas l'API publique.

## Prototype natif

`tools/spotify-probe` teste séparément la connexion native, les métadonnées et,
avec `--play`, trois secondes de lecture. Il s'appuie sur
[librespot 0.8.0](https://github.com/librespot-org/librespot/tree/v0.8.0), sous licence
MIT. Ce candidat utilise un moteur audio Rust et les protocoles Spotify ; son
fonctionnement avec ce compte reste à tester. Ce n'est pas une validation du
mode hors ligne, de la qualité sans perte ou d'une connexion durable.

Le fichier de session est fourni uniquement en local. La CI compile sans compte,
sans captures et sans secrets Spotify. Elle ne teste pas la lecture effective.

Premier build : incompatibilité de traits entre `vergen-gitcl 1.0.8` et
`vergen 9.1.0`, dépendances de construction de librespot. Le verrouillage conserve
`vergen 9.0.6`, compatible avec la génération de métadonnées utilisée par librespot.

Ces chemins ont été lus dans Fiddler. Aucun en-tête d'authentification, identifiant
de compte, titre écouté ni contenu de capture n'est publié.

## Sources officielles consultées

- [SDK de lecture Spotify](https://developer.spotify.com/documentation/web-playback-sdk) :
  crée un lecteur Spotify Connect dans un navigateur compatible. La page cite
  Edge, mais ne garantit pas explicitement la prise en charge de Tauri/WebView2.
- [Connexion PKCE](https://developer.spotify.com/documentation/web-api/tutorials/code-pkce-flow) :
  flux prévu pour les clients qui ne peuvent pas conserver de secret applicatif.
- [Quotas](https://developer.spotify.com/documentation/web-api/concepts/quota-modes) :
  restrictions du mode développement et utilisateurs autorisés. Une API publique
  ne doit pas être supposée équivalente aux services internes du client Spotify.
- [Prérequis Tauri Windows](https://v2.tauri.app/start/prerequisites/) : Rust MSVC,
  outils C++ Microsoft et WebView2 nécessaires pour compiler localement.

## Questions à résoudre par le prototype

1. Quelles requêtes chargent l'accueil, les playlists et la recherche ?
2. Comment établir et renouveler une session propre à Fastify ? Un jeton capturé
   expirant n'est pas une solution de connexion durable.
3. Quel mécanisme lit réellement l'audio dans Tauri, sans dépendre d'un client
   Spotify ouvert ? Les métadonnées et commandes Connect seules ne suffisent pas.
4. Quelles fonctions restent accessibles, notamment paroles et hors ligne ?

À ce stade, la parité et l'autonomie audio ne sont pas établies. Ne pas contourner
les protections de lecture ni présenter des commandes distantes comme un moteur audio.
