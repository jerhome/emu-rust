# ADR 0002 — Stack graphique : winit + glutin + glow + egui

Statut : accepté (2026-11-19)

## Contexte

- Les cores 16 bits envoient des images logicielles (callback vidéo) : il suffit de les téléverser dans une texture.
- Plusieurs cores 32 bits (beetle-psx HW, etc.) exigent un **contexte OpenGL ou Vulkan fourni par le frontend**
  (`RETRO_ENVIRONMENT_SET_HW_RENDER`) : le frontend doit maîtriser ce contexte.
- macOS Intel plafonne à OpenGL 4.1 core ; WSLg fournit OpenGL via Mesa (d3d12) ; noVNC/Xvfb via llvmpipe.

## Décision

- Fenêtre et événements : `winit`. Contexte GL : `glutin`. Appels GL : `glow`. Interface : `egui` + `egui_glow`.
- Pas d'`eframe` : il possède la boucle et le contexte, ce qui gêne le rendu matériel des cores et le pilotage du timing.
- Cible minimale **OpenGL 3.3 core**, rien au-delà de 4.1. Vulkan éventuellement en phase 7.
- Le rendu de l'émulation (texture + mise à l'échelle entière / lissage) est dans `emu-runtime` ; `emu-ui` dessine par-dessus.

## Conséquences

- Plus de code d'intégration qu'avec eframe, en échange du contrôle total du contexte et de la boucle.
- Les shaders (CRT, etc.) viendront plus tard sous forme de passes GLSL simples.
