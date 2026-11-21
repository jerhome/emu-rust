# Variantes d'icône (7.1, ADR 0023 §2)

Quatre propositions, formes pleines sans dégradé, aux couleurs de l'interface (fond sombre `#1B1B1F`, accent
`#285AA0`, focus `#96C8FF`, ambre `#FFBE50`). Aucune marque, aucune manette d'un fabricant.

| Variante | Idée |
|---|---|
| A `variante-a.svg` | Cartouche générique, triangle « lecture » sur l'étiquette |
| B `variante-b.svg` | Croix directionnelle et deux boutons ronds sur fond bleu |
| C `variante-c.svg` | Écran affichant un triangle « lecture » en gros pixels |
| D `variante-d.svg` | Grille de jaquettes de la bibliothèque, une tuile sélectionnée |

`planche.png` : chaque variante en 256, 64, 32 et 16 px, sur fond sombre et clair. Régénération :
`docs/design/icone/planche.sh docs/design/icone <dossier temporaire>` (ImageMagick seul, rendu SVG interne ;
`rsvg-convert` et Inkscape ne sont pas installés dans le conteneur de développement).

Le choix revient à l'utilisateur ; la variante retenue devient `packaging/icon/emu-rust.svg`.
