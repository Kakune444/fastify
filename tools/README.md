# Diagnostics locaux

Ces outils servent au développement. Ils ne font pas partie des fonctions de
l'application et ne transmettent pas de données au dépôt ou à la CI.

## Connexion, catalogue et audio

Compiler `spotify-probe` sur Windows avec les prérequis Rust MSVC :

```powershell
cargo build --release --locked --manifest-path tools/spotify-probe/Cargo.toml
```

Après une connexion normale à Spotisloth, vérifier la réutilisation de sa session
et la première page de bibliothèque :

```powershell
./tools/spotify-probe/target/release/spotify-probe.exe --stored --library
```

Seule l'entrée du coffre appartenant à Spotisloth est lue. La sortie affiche des
résultats et des compteurs, sans identifiants ni jetons. `--library` est en lecture
seule. Si `SPOTISLOTH_DIAGNOSTIC_DIR` est défini, la réponse d'images est enregistrée
dans ce dossier ; utiliser uniquement un dossier local ignoré comme `.local`.

Le mode historique accepte aussi un fichier JSON local avec `accessToken` et un
`trackUri` facultatif. Ajouter `--play` lance alors le titre pendant trois secondes,
puis arrête le lecteur. Un jeton capturé peut expirer ; ce mode n'établit pas la
validité de la connexion normale. Ne jamais publier ce fichier.

## Spotify Connect

Avec Spotify ouvert sur un autre appareil du même compte, ce diagnostic enregistre
un récepteur temporaire et attend les mises à jour de la liste des appareils :

```powershell
./tools/spotify-probe/target/release/spotify-probe.exe --stored --connect
```

Il ne lance aucun morceau et ne transfère pas la lecture. Il affiche seulement
des compteurs (`CONNECT_DEVICES_OK`) ; il ferme sa session à la fin. Les transferts
et commandes du lecteur doivent aussi être vérifiés dans l’exécutable Tauri.

## Ressources

Voir le [protocole de mesure](../docs/PERFORMANCE.md) pour `Measure-Client.ps1`.
