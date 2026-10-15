# État du prototype

Date de vérification : 15 octobre 2026. Une fonction implémentée reste à vérifier
dans l'exécutable avant d'être déclarée opérationnelle.

| Élément                                | État et preuve                                                                                  |
| -------------------------------------- | ----------------------------------------------------------------------------------------------- |
| Dépôt public                           | Créé, commits et workflow Windows disponibles                                                   |
| Connexion native avec session capturée | Vérifiée : `SESSION_OK`                                                                         |
| Métadonnées natives                    | Vérifiées : `METADATA_OK`                                                                       |
| Lecture audio native                   | Démarrage et arrêt après trois secondes vérifiés : `PROBE_OK`                                   |
| Accueil interne                        | Rejeu HTTP 200, y compris sans identifiant de suivi                                             |
| Recherche complète interne             | Rejeu HTTP 200 de `searchDesktop`                                                               |
| Connexion OAuth et coffre Windows      | Connexion terminée par l'utilisateur ; reconnexion après redémarrage vérifiée                   |
| Interface et lecteur Tauri             | Exécutable release compilé ; accueil, recherche et lecteur testés sous Windows                  |
| Bibliothèque                           | Noms, pochettes et deuxième page vérifiés : 27 puis 57 playlists affichées                      |
| Albums et playlists                    | Album de 16 titres et playlist de 36 titres affichés ; pagination à vérifier                    |
| Pause et position                      | Pause et déplacement à 25 % vérifiés dans l'application                                         |
| Volume et file locale                  | Volume réglé à 20 %, titre suivant et conservation après consultation d’un autre album vérifiés |
| Édition des playlists et favoris       | À implémenter                                                                                   |
| Artistes, podcasts, livres audio       | Recherche partielle, pages et lecture à compléter                                               |
| Aléatoire, répétition et file Connect  | À implémenter ; contrôles indisponibles signalés                                                |
| Paroles, hors ligne, médias Windows    | À étudier et implémenter                                                                        |
| Comparaison de ressources équivalentes | Non effectuée ; relevé préliminaire documenté                                                   |
| Installateur et distribution publique  | En attente d'une version utilisable                                                             |

## Limites techniques connues

Les requêtes internes et leurs hashes peuvent changer côté Spotify. Le frontend
n'inclut pas de runtime Node.js en production. Le moteur audio est Rust ; WebView2
affiche uniquement l'interface. Les données et secrets restent côté Rust, sauf les
métadonnées utiles à l'affichage. Les captures et fichiers de diagnostic restent
exclus du dépôt et des artefacts CI.

L'expiration est gérée par librespot pour les sessions natives. Le mode de diagnostic
par fichier dépend d'un jeton temporaire. La recherche est temporisée à 350 ms et
ignore les réponses devenues obsolètes ; l'annulation réseau complète est à ajouter.

La bibliothèque affiche actuellement les playlists avec le nom technique de leur propriétaire ; les dossiers, albums et artistes
suivis ne sont pas encore intégrés. La lecture de collection charge 40 titres par
page. La file locale n'enchaîne pas encore automatiquement les pages suivantes.
Le renouvellement après expiration réelle et la récupération d'une session révoquée
restent à éprouver ; la reconnexion après redémarrage ne suffit pas à les valider.

## Version vérifiée

Code : `bb0cfc0`. [Compilation Windows et tests](https://github.com/Kakune444/fastify/actions/runs/38678071244) réussis : cinq tests TypeScript et deux tests Rust. L’exécutable de cette compilation a été relancé localement et les corrections de bibliothèque et de file ont été vérifiées dans cette version.
