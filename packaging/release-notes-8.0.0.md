> **Avant de lancer cette version : copiez le dossier des données d'emu-rust.** La 8.0 fait passer la base de la
> bibliothèque au schéma v8, et une version 7.x refuse ensuite de l'ouvrir. Pour revenir en arrière : fermez emu-rust
> et restaurez votre copie. Aucune restauration automatique n'existe.

**emu-rust @VERSION@** : ajoutez vous-même un système que libretro propose mais qu'emu-rust n'a pas testé
(écran Cores, « Ajouter un système non testé… »), avec essai à blanc au premier lancement d'un jeu, témoin de
lancement et réglages par système. Exemple d'essai réel prévu : l'Atari Lynx.

Ces systèmes sont **non testés** : rien n'est garanti, l'étiquette reste affichée. Les cores qui demandent le rendu
matériel (prévu en 9.0), les disques seuls (8.4) et l'arcade (8.6) ne sont pas proposés. Aucun core, BIOS ni ROM
n'est fourni. Guide d'essai : `docs/guide-essai-8.0.md`. Le détail et les limites connues suivent.

---
