# winit 0.30.13 (copie locale modifiee pour AutoGameSoftwareSync Android)

Copie de la version 0.30.13 publiee sur crates.io (licence Apache-2.0, voir LICENSE), utilisee par la seule application
Android via `[patch.crates-io]` dans `android/Cargo.toml`. L'application PC n'est pas concernee.

Deux fichiers changent, pour les consoles portables (manette integree) ; chaque bloc modifie est marque
« AutoGameSoftwareSync (console portable) » dans le code :

- `src/platform_impl/android/keycodes.rs` : les boutons de la manette deviennent des touches d'interface
  (A = Entree, B et le bouton Retour du systeme = Echap, X = F1, Y = F2, L1 = Page precedente, R1 = Page suivante,
  Start = F3, Select = F4) au lieu de `Key::Unidentified` (ignore par egui).
- `src/platform_impl/android/mod.rs` : les axes de la manette (croix « chapeau » AXIS_HAT_X / Y et stick gauche) sont
  traduits en touches fleches (`handle_pad_axes`), au lieu d'etre ignores (ou transformes en toucher pour le stick).
  Ces touches sont de VRAIES entrees (`is_synthetic: false`) : egui-winit ignore volontairement tout appui marque
  synthetique (issue winit 3543), seul le relachement arriverait a l'interface. Quand aucune direction n'est emise ni
  enfoncee, l'evenement est rendu a Android (`InputStatus::Unhandled`) qui peut alors fabriquer lui-meme ses touches
  DPAD de secours ; des qu'une direction est tenue, il est « gere » (jamais de doublon).
  La logique pure des seuils est reprise a l'identique dans `android/src/pad.rs` (`dirs_from_axes`), testee sur PC :
  garder les deux copies identiques.

Tout le reste est identique a la version publiee. Pour une mise a jour de winit : reprendre ces deux changements.
