# État du prototype

Date de vérification : 17 octobre 2026. Une fonction implémentée reste à vérifier
dans l'exécutable avant d'être déclarée opérationnelle.

| Élément                                | État et preuve                                                                                                                                                                                                                                 |
| -------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Dépôt public                           | Créé, commits et workflow Windows disponibles                                                                                                                                                                                                  |
| Connexion native avec session capturée | Vérifiée : `SESSION_OK`                                                                                                                                                                                                                        |
| Métadonnées natives                    | Vérifiées : `METADATA_OK`                                                                                                                                                                                                                      |
| Lecture audio native                   | Démarrage et arrêt après trois secondes vérifiés : `PROBE_OK`                                                                                                                                                                                  |
| Accueil interne                        | Rejeu HTTP 200, y compris sans identifiant de suivi                                                                                                                                                                                            |
| Recherche complète interne             | Rejeu HTTP 200 de `searchDesktop`                                                                                                                                                                                                              |
| Connexion OAuth et coffre Windows      | Connexion terminée par l'utilisateur ; reconnexion après redémarrage vérifiée                                                                                                                                                                  |
| Interface et lecteur Tauri             | Exécutable release compilé ; accueil, recherche et lecteur testés sous Windows                                                                                                                                                                 |
| Bibliothèque                           | Noms, pochettes et deuxième page vérifiés : 27 puis 57 playlists affichées                                                                                                                                                                     |
| Albums et playlists                    | Album de 16 titres et playlist de 36 titres affichés ; pagination à vérifier                                                                                                                                                                   |
| Pause et position                      | Pause et déplacement à 25 % vérifiés dans l'application                                                                                                                                                                                        |
| Volume et file locale                  | Volume réglé à 20 %, titre suivant et conservation après consultation d’un autre album vérifiés                                                                                                                                                |
| Favoris                                | Ajout puis retrait confirmés par relecture sur un compte réel ; état initial restauré                                                                                                                                                          |
| Édition des playlists                  | Création privée, nom/description, ajout, retrait d’un doublon, déplacement et conflit vérifiés par le moteur ; formulaires, menu et recherche du sélecteur vérifiés dans Tauri                                                                 |
| Artistes, podcasts, livres audio       | Recherche partielle, pages et lecture à compléter                                                                                                                                                                                              |
| Aléatoire et répétition                | Aléatoire et titre suivant vérifiés dans Tauri ; répétition du titre à sa fin et retour au début de la playlist vérifiés                                                                                                                       |
| File locale                            | Ajout prioritaire, doublon distinct, métadonnées différées, enchaînement après fin et glisser-déposer Windows vérifiés                                                                                                                         |
| File et appareils Connect              | Deux appareils détectés ; commandes et file vérifiées dans Tauri. Transferts aller-retour vérifiés en lecture et en pause, avec conservation des modes et des 40 titres suivants ; volume et ajout/retrait d’un doublon confirmés par la sonde |
| Paroles, hors ligne, médias Windows    | À étudier et implémenter                                                                                                                                                                                                                       |
| Comparaison de ressources équivalentes | Non effectuée ; relevé préliminaire documenté                                                                                                                                                                                                  |
| Installateur et distribution publique  | En attente d'une version utilisable                                                                                                                                                                                                            |

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
page. Le nouveau lecteur laisse librespot résoudre et paginer le contexte Spotify ;
la file affichée suit les prochains titres publiés par l’appareil actif.
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

## Spotify Connect et file partagée

Le même moteur Rust assure la lecture audio et le récepteur Connect. La réponse
d’inscription et les mises à jour du serveur alimentent l’interface ; elle ne
demande pas l’état Spotify en boucle. L’identifiant de cet ordinateur est conservé
localement pour retrouver le même appareil après redémarrage. La session conservée
sous l’ancien nom Fastify est reprise dans le coffre Spotisloth après connexion.

La liste d’appareils, le titre, la position, les modes et la file suivent l’appareil
actif. Le moteur gère aussi les fins de morceaux ; l’interface ne fait plus avancer
une seconde file locale. Elle affiche 40 titres à la fois parmi les prochains titres
publiés par Spotify (jusqu’à 80 pour le récepteur librespot). Les métadonnées
manquantes restent chargées au besoin, par lots limités.
Les séparateurs internes restent dans le protocole et sont exclus des lignes de
titres. L’affichage s’arrête au premier séparateur de cycle pour ne pas présenter
les répétitions futures comme des ajouts. Les ajouts manuels et la suite du contexte
ont leurs sections habituelles.

Les commandes capturent l’appareil visé. Les déplacements et retraits utilisent la
révision de file et une occurrence précise : une ancienne ligne ne peut pas cibler
un autre doublon après une mise à jour. Les restrictions de l’appareil actif sont
appliquées dans le lecteur et vérifiées côté Rust. Une commande réseau dont la
réponse est perdue n’est pas renvoyée automatiquement ; le message demande de
vérifier la lecture. La file partagée n’écrit pas dans les playlists.

Un transfert attend la confirmation de l’appareil et du morceau. Le récepteur local
attend aussi la préparation de son contexte. Si la destination reconstruit un autre
ordre, le client restaure la portion de file publiée par la source, avec la révision
courante de la destination. L’instantané reste borné et côté Rust. Les modes, le
contexte et les ajouts manuels sont contrôlés avant cette restauration ; un changement
concurrent l’interrompt. La position et l’état lecture/pause sont également rétablis
si nécessaire, après confirmation de la destination.

La copie locale de librespot-connect 0.8.0 conserve sa licence et son empreinte
d’origine. Son [adaptation documentée](../vendor/librespot-connect/SPOTISLOTH.md)
publie les états déjà reçus lors de l’inscription, des commandes et des mises à jour.
Elle ajoute un canal borné en mémoire, sans nouveau trafic réseau.
Elle corrige aussi l’ordre de désactivation lors d’un transfert sortant pour que
le récepteur ne reprenne pas la main au moment d’arrêter sa lecture locale.

Le gestionnaire natif de dépôt Tauri est désactivé dans la fenêtre Windows pour
permettre le glisser-déposer HTML des lignes. Ce réglage suit la
[configuration officielle Tauri](https://v2.tauri.app/reference/config/#windowconfig).

L’implémentation utilise les appareils enregistrés sur le compte. La découverte
locale par mDNS, l’appairage Cast et les sessions Jam ne sont pas implémentés.
Les appareils physiques, les restrictions propres à chaque modèle et une longue
perte réseau nécessitent des vérifications supplémentaires.
La conservation de l’ordre porte sur le tampon publié par l’appareil source ; elle
ne garantit pas le même tirage aléatoire au-delà de ce tampon.

## Version vérifiée

Code compilé : `670abfb`.
[Compilation Windows et tests](https://github.com/Kakune444/spotisloth/actions/runs/39019549135)
réussis : 24 tests TypeScript et 22 tests Rust. Les vérifications utilisent le
compte local, avec Spotify Windows et le récepteur Spotisloth. Elles ne constituent
pas une validation de tous les appareils Connect.

Dans Tauri (`35d04f1`), la lecture native apparaît dans Spotify et les commandes de
pause/reprise suivent l’appareil actif. La position distante, le choix d’un titre
de la file et son déplacement au clavier ont été vérifiés ; l’ordre déplacé a été
restauré. La version finale `670abfb` a été lancée sous Windows : reconnexion,
accueil, titre distant, bandeau de lecture, sélecteur d’appareils et affichage de la
file partagée sont vérifiés.

La sonde de `670abfb`, qui utilise le même service Rust, confirme les transferts
Spotify → Spotisloth → Spotify en lecture et en pause. Elle vérifie le même morceau,
la position à deux secondes près, les modes aléatoire/répétition et les 40 premières
occurrences à suivre. Le volume est modifié puis restauré avec confirmation serveur.
Un doublon est ajouté puis son occurrence précise retirée ; les autres titres sont
conservés. Aucun titre de test ne reste dans la file et la lecture a repris sur
l’appareil d’origine.

Avant le remplacement de la file locale par Connect, l’aléatoire sur le contexte complet, le passage au titre affiché dans la file,
la répétition d’un titre après sa fin réelle et le retour de la fin d’une playlist
à son début ont été vérifiés dans `162e4cf`. L’ajout prioritaire d’une occurrence
en double puis le retour à la sélection ont également été vérifiés. La sonde de
cette version a relu les liens de tous les favoris et leur état sans mutation.

Les essais de mutations de favoris et de playlists restent ceux de `2886e1e`,
avec le même service Rust d’écriture. Les formulaires et le sélecteur avaient été
vérifiés dans `8aa06b2`, puis le visuel et la pagination des favoris dans `327a6c4`.
