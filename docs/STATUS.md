# État du prototype

Date de vérification : 15 octobre 2026. Une fonction implémentée reste à vérifier
dans l'exécutable avant d'être déclarée opérationnelle.

| Élément                                | État et preuve                                                |
| -------------------------------------- | ------------------------------------------------------------- |
| Dépôt public                           | Créé, commits et workflow Windows disponibles                 |
| Connexion native avec session capturée | Vérifiée : `SESSION_OK`                                       |
| Métadonnées natives                    | Vérifiées : `METADATA_OK`                                     |
| Lecture audio native                   | Démarrage et arrêt après trois secondes vérifiés : `PROBE_OK` |
| Accueil interne                        | Rejeu HTTP 200, y compris sans identifiant de suivi           |
| Recherche complète interne             | Rejeu HTTP 200 de `searchDesktop`                             |
| Connexion OAuth et coffre Windows      | Implémentés, parcours utilisateur à vérifier                  |
| Interface et lecteur Tauri             | Implémentés, compilation et essais Windows en attente         |
| Bibliothèque                           | Première page de rootlist implémentée, à vérifier             |
| Albums et playlists                    | Chargement par pages de 40 titres implémenté, à vérifier      |
| Pause, position, volume, file locale   | Implémentés, intégration à vérifier                           |
| Édition des playlists et favoris       | À implémenter                                                 |
| Artistes, podcasts, livres audio       | Recherche partielle, pages et lecture à compléter             |
| Aléatoire, répétition et file Connect  | À implémenter ; contrôles indisponibles signalés              |
| Paroles, hors ligne, médias Windows    | À étudier et implémenter                                      |
| Comparaison de ressources équivalentes | Non effectuée                                                 |
| Installateur et distribution publique  | En attente d'une version utilisable                           |

## Limites techniques connues

Les requêtes internes et leurs hashes peuvent changer côté Spotify. Le frontend
n'inclut pas de runtime Node.js en production. Le moteur audio est Rust ; WebView2
affiche uniquement l'interface. Les données et secrets restent côté Rust, sauf les
métadonnées utiles à l'affichage. Les captures et fichiers de diagnostic restent
exclus du dépôt et des artefacts CI.

L'expiration est gérée par librespot pour les sessions natives. Le mode de diagnostic
par fichier dépend d'un jeton temporaire. La recherche est temporisée à 350 ms et
ignore les réponses devenues obsolètes ; l'annulation réseau complète est à ajouter.
