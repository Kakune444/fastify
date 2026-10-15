# État du prototype

Date de vérification : 16 octobre 2026. Une fonction implémentée reste à vérifier
dans l'exécutable avant d'être déclarée opérationnelle.

| Élément                                | État et preuve                                                                                                                                                                 |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Dépôt public                           | Créé, commits et workflow Windows disponibles                                                                                                                                  |
| Connexion native avec session capturée | Vérifiée : `SESSION_OK`                                                                                                                                                        |
| Métadonnées natives                    | Vérifiées : `METADATA_OK`                                                                                                                                                      |
| Lecture audio native                   | Démarrage et arrêt après trois secondes vérifiés : `PROBE_OK`                                                                                                                  |
| Accueil interne                        | Rejeu HTTP 200, y compris sans identifiant de suivi                                                                                                                            |
| Recherche complète interne             | Rejeu HTTP 200 de `searchDesktop`                                                                                                                                              |
| Connexion OAuth et coffre Windows      | Connexion terminée par l'utilisateur ; reconnexion après redémarrage vérifiée                                                                                                  |
| Interface et lecteur Tauri             | Exécutable release compilé ; accueil, recherche et lecteur testés sous Windows                                                                                                 |
| Bibliothèque                           | Noms, pochettes et deuxième page vérifiés : 27 puis 57 playlists affichées                                                                                                     |
| Albums et playlists                    | Album de 16 titres et playlist de 36 titres affichés ; pagination à vérifier                                                                                                   |
| Pause et position                      | Pause et déplacement à 25 % vérifiés dans l'application                                                                                                                        |
| Volume et file locale                  | Volume réglé à 20 %, titre suivant et conservation après consultation d’un autre album vérifiés                                                                                |
| Favoris                                | Ajout puis retrait confirmés par relecture sur un compte réel ; état initial restauré                                                                                          |
| Édition des playlists                  | Création privée, nom/description, ajout, retrait d’un doublon, déplacement et conflit vérifiés par le moteur ; formulaires, menu et recherche du sélecteur vérifiés dans Tauri |
| Artistes, podcasts, livres audio       | Recherche partielle, pages et lecture à compléter                                                                                                                              |
| Aléatoire, répétition et file Connect  | À implémenter ; contrôles indisponibles signalés                                                                                                                               |
| Paroles, hors ligne, médias Windows    | À étudier et implémenter                                                                                                                                                       |
| Comparaison de ressources équivalentes | Non effectuée ; relevé préliminaire documenté                                                                                                                                  |
| Installateur et distribution publique  | En attente d'une version utilisable                                                                                                                                            |

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

Les modifications de playlists utilisent la révision lue sur le serveur. Un conflit
déclenche un rechargement ; une requête d’écriture dont la réponse est perdue n’est
jamais renvoyée automatiquement. Les droits renvoyés par Spotify priment sur le
statut de propriétaire. Le retrait vise une position précise pour conserver les
autres occurrences du même titre. Les métadonnées indisponibles conservent leur
position dans la liste.

La liste des favoris est paginée. Le service collection peut renvoyer des éléments
supprimés ou d’autres types : les pages sans titre sont sautées dans une limite de
dix requêtes par chargement. Les playlists proposées pour l’ajout sont chargées
seulement à l’ouverture du sélecteur ; leur contenu n’est pas préchargé.

## Vérification des mutations Spotify

La sonde utilise la session de Fastify dans le coffre Windows. Le test de favori
choisit un titre initialement absent, confirme son ajout, puis confirme son retrait.
Le test de playlist crée un élément privé temporaire, vérifie les opérations et
le retire de la bibliothèque à la fin. Il vérifie aussi qu’une ancienne révision
est refusée et qu’un seul doublon est retiré. Aucune donnée de compte ne passe par
la compilation GitHub.

Le client Tauri a affiché les vrais Titres likés et leur état sauvegardé. Les
fenêtres de création et de modification ont été ouvertes ; le menu d’un titre
ouvre un sélecteur ne contenant que des playlists modifiables. Sa recherche a
été testée. Les mutations complètes sont vérifiées par la sonde qui partage le
même service Rust ; tous les clics d’écriture n’ont pas été rejoués dans l’interface.

## Version vérifiée

Code : `327a6c4`. [Compilation Windows et tests](https://github.com/Kakune444/fastify/actions/runs/38822841513)
réussis : huit tests TypeScript et dix tests Rust. L’exécutable release de cette
compilation a été relancé localement et s’est reconnecté au coffre Windows. Les
Titres likés, leur état, le visuel violet et le passage à la deuxième page ont été
vérifiés dans cette version : les titres 39 et suivants s’affichent après les 38
titres de la première page.
Les formulaires et le sélecteur ont été vérifiés dans `8aa06b2`, dont le code
d’interface est identique hormis la correction des deux sélecteurs CSS de favoris.
Les essais de mutations ont été répétés avec la sonde de `2886e1e`, qui partage le
même service Rust que `327a6c4`.
