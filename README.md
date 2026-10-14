# Hunger Overlay

Affiche les objectifs de votre **quest tracker [Hunger Companion](https://hunger-companion.com)** directement
**par-dessus la carte du jeu Hunger**, en transparence.

- Un seul `.exe` (≈ 1 Mo), sans installation ni runtime, sans droits administrateur.
- Ouvrez la carte en jeu : vos objectifs apparaissent aux bons endroits et suivent le zoom et le déplacement.
- La détection est **visuelle** : aucune touche à configurer. La touche carte (M par défaut) ne sert que d'accélérateur.

## Utilisation

1. Sur hunger-companion.com, connectez-vous, puis **Compte → Sécurité → Clé overlay → Générer**.
2. Lancez `HungerOverlay.exe` et collez la clé dans la fenêtre de paramètres.
3. Jouez en mode **fenêtré plein écran (sans bordure)** : le plein écran exclusif masque tout overlay.
4. Suivez des quêtes sur `hunger-companion.com/tracker`, puis ouvrez la carte en jeu.

L'icône près de l'horloge donne accès aux paramètres, au rafraîchissement et à la commande Quitter.

## Ce que fait (et ne fait pas) l'appli

- Elle **lit uniquement l'écran** (quelques pixels pour reconnaître la carte et sa position) et dessine dans une
  fenêtre transparente qui ne capte pas les clics. Elle ne lit pas la mémoire du jeu, n'injecte rien, ne modifie
  aucun fichier du jeu et ne simule aucune entrée.
- Elle n'envoie au site que votre clé d'API. Le site renvoie vos quêtes suivies, et rien ne part du jeu.
- Requêtes : la liste des objectifs une fois par lancement (mise en cache), le tracker à l'ouverture de la carte
  (au plus une fois toutes les 30 s), sans aucune interrogation en arrière-plan.
- La clé est chiffrée sur votre PC (DPAPI Windows) dans `%APPDATA%\HungerOverlay\`.

> Easy Anti-Cheat : l'appli appartient à la même catégorie que les overlays externes (Discord, Overwolf…). Elle n'est
> ni affiliée à l'éditeur du jeu ni approuvée par lui : utilisation à vos risques.

## Développement

- Rust stable (MSVC). `cargo build --release` produit `target/release/HungerOverlay.exe`.
- `tools/make_assets.py <atlas>` régénère les vignettes de référence des cartes et l'icône.
- `tools/mock_site.py <HungerDB/export>` lance un faux site local (contrat v1). Pour s'en servir, mettez
  `"base_url": "http://127.0.0.1:8787"` dans `%APPDATA%\HungerOverlay\config.json`.
- Les mesures de l'interface carte (zoom, déplacement, panneau) sont documentées dans
  `HungerDB/docs/overlay/MAP_UI_MEASUREMENTS.md`.
