# Version 10 (mobile) — grill

Méthode mattpocock (décision de l'utilisateur du 2026-11-26) : grill-with-docs → to-spec → to-tickets → implement →
code-review → retro. Questionnaire du tour 1 : <https://claude.ai/artifact/W2HkvqkjgtxAUgQV92hFec>.

Convention de l'utilisateur : « plus tard » = 10.x, « bien plus tard » = 11.x ; rien n'est acté pour ces numéros.

## Tour 1 — décisions racines (2026-11-26)

| # | Question | Décision | Pour plus tard |
|---|---|---|---|
| 1 | Jeu local ou à distance | émulation locale sur l'appareil, hors ligne | 11.x : noVNC pour les jeux lourds |
| 2 | Forme de l'application | édition web (WASM) installable sur l'écran d'accueil | 10.x : iOS natif ; 11.x : Android natif |
| 3 | Appareil de test | iPhone 13 mini | — |
| 4 | Systèmes visés | cartouches 8/16 bits + Game Boy / GBA | 11.x : tout ce que fait le bureau |
| 5 | Contrôles | manette Bluetooth d'abord, boutons tactiles en secours | — |
| 6 | Calendrier | après la 9.0 ; grill et spec (to-spec) peuvent se faire en avance | — |

## Tour 2 — frontière (à poser)

- Stockage des ROMs et des sauvegardes sur l'iPhone (fichiers choisis à chaque fois, bibliothèque persistante dans le
  navigateur, import depuis le PC) et sauvegarde de ces données.
- Hors ligne et hébergement (application web installable, où elle est servie).
- Partage de code avec l'application de bureau (crates communs, ce qui reste propre au bureau).
- Faits à faire chercher par un sous-agent avant de poser les questions qui en dépendent : contraintes de Safari iOS sur
  iPhone 13 mini (WASM, threads, audio, manette Bluetooth, stockage persistant, plein écran), licences des cores
  libretro compilés en WASM et leur distribution.
- Jaquettes et métadonnées hors ligne.
