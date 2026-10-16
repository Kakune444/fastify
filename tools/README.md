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

## Ressources

Voir le [protocole de mesure](../docs/PERFORMANCE.md) pour `Measure-Client.ps1`.
