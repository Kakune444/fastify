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
| Aléatoire et répétition                | Aléatoire et titre suivant vérifiés dans Tauri ; répétition du titre à sa fin et retour au début de la playlist vérifiés                                                       |
| File locale                            | Ajout prioritaire, doublon distinct, métadonnées différées, enchaînement après fin et glisser-déposer Windows vérifiés                                                         |
| File et appareils Connect              | Synchronisation avec les autres appareils à implémenter                                                                                                                        |
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
page. La nouvelle file prépare les liens du contexte complet lors d’une sélection
explicite ; elle ne dépend plus des seuls titres de la page affichée.
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

La sonde utilise la session de Spotisloth dans le coffre Windows. Le test de favori
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

## File et modes du lecteur

La préparation lit les liens des titres par lots de 1 000, avec contrôle de révision
pour les playlists. Les métadonnées restent chargées au besoin, avec quatre lectures
concurrentes au maximum. La file affiche 40 éléments avant de proposer la suite.
Une sélection abandonnée arrête la pagination après la requête déjà en cours.
La limite de préparation est de 100 pages et 100 000 liens ; une réponse incohérente
est refusée. La recherche utilise les résultats affichés comme sélection.

Chaque occurrence possède son propre identifiant, même lorsque son lien Spotify
est identique à un autre. La file est locale et n’écrit pas dans la playlist ; ses
modes et son contenu ne sont pas conservés après fermeture. Le retour précédent
suit les titres effectivement écoutés, avec un historique borné à 200 entrées.
L’aléatoire mélange les occurrences restantes du contexte, sans ajout de recommandations
Smart Shuffle et sans reproduction de l’algorithme personnalisé de Spotify.

Chaque chargement audio possède aussi un identifiant de sélection. Le moteur associe
les accusés de chargement librespot à cette sélection ; une ancienne fin de morceau
ne peut pas faire avancer une nouvelle lecture du même lien. Les commandes de
lecture, pause et déplacement sont également rattachées à la sélection active.

Le gestionnaire natif de dépôt Tauri est désactivé dans la fenêtre Windows pour
permettre le glisser-déposer HTML des lignes. Ce réglage suit la
[configuration officielle Tauri](https://v2.tauri.app/reference/config/#windowconfig).

Les tests du lecteur couvrent les occurrences en double, la priorité des ajouts
manuels, les modes de répétition, l’historique et les déplacements. Le retrait
précis d’une occurrence est testé dans le modèle de file ; son clic n’a pas été
rejoué dans l’application Windows.

## Version vérifiée

Code : `ae7d155`. [Compilation Windows et tests](https://github.com/Kakune444/spotisloth/actions/runs/38952166028)
réussis : vingt tests TypeScript et douze tests Rust. L’exécutable release de cette
compilation a été installé localement. La reconnexion au coffre Windows, les
favoris, le chargement sans débordement du bouton et le démarrage audio ont été
vérifiés. Le déplacement d’un titre dans la file, la conservation
du morceau en cours et la lecture directe du titre déplacé ont été vérifiés dans
`bf6a406` ; seul le style de chargement du bouton de lecture a changé depuis.

L’aléatoire sur le contexte complet, le passage au titre affiché dans la file,
la répétition d’un titre après sa fin réelle et le retour de la fin d’une playlist
à son début ont été vérifiés dans `162e4cf`. L’ajout prioritaire d’une occurrence
en double puis le retour à la sélection ont également été vérifiés. La sonde de
cette version a relu les liens de tous les favoris et leur état sans mutation.

Les essais de mutations de favoris et de playlists restent ceux de `2886e1e`,
avec le même service Rust d’écriture. Les formulaires et le sélecteur avaient été
vérifiés dans `8aa06b2`, puis le visuel et la pagination des favoris dans `327a6c4`.
