# Mesures de performance

## Première observation

Le 15 octobre 2026, la somme des working sets des neuf processus Spotify ouverts
était d'environ 1 425 Mio. C'est une observation ponctuelle d'une session existante,
pas une mesure de démarrage ni une consommation privée et exclusive. Les pages
mémoire partagées peuvent être comptées plusieurs fois.

## Comparaison à réaliser

Tester Spotify et Fastify release sur le même PC, compte et réseau, à tour de rôle.
Fermer les applications avant les mesures à froid. Faire cinq passages par scénario.

| Scénario | Mesures |
| --- | --- |
| Démarrage à froid puis repos de 30 s | Temps jusqu'à interaction, mémoire privée, working set, CPU |
| Accueil et bibliothèque | Délai d'affichage et nombre d'appels réseau |
| Recherche identique | Latence jusqu'aux résultats, annulation des recherches précédentes |
| Playlist longue | Réactivité au défilement, mémoire après parcours |
| Lecture pendant 10 minutes | Coupures, CPU, mémoire et croissance des caches |

Inclure le processus Rust et **tous ses descendants WebView2** dans la mesure Fastify.
Ne pas comparer un shell vide avec un Spotify lisant de la musique. Indiquer les
fonctions absentes et conserver les résultats bruts localement.

Objectifs provisoires : réduire sensiblement mémoire et latences, sans régression
fonctionnelle. Les seuils définitifs seront fixés après le premier prototype réel.
