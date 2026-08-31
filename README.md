# herd-expose

Exposé / Mission Control pour [herdr](https://github.com/herdrdev/herdr) : sur un raccourci, un popup plein écran affiche tous les panes du workspace courant sous forme de tuiles (aperçu du contenu + statut d'agent), groupés par tab. Sélectionner une tuile bascule le focus sur le pane.

Statut : prototype (v0.2). Aperçus ANSI stylés, grille adaptative. Reste à valider en session interactive : le focus posé par le plugin survit à la fermeture du popup.

## Installation (développement)

```bash
cargo build --release
herdr plugin link /path/to/herd-expose
```

Raccourci, dans la config herdr :

```toml
[[keys.command]]
key = "prefix+e"
type = "plugin_action"
command = "vjeantet.expose.open"
description = "exposé"
```

Test manuel sans raccourci :

```bash
herdr plugin pane open --plugin vjeantet.expose --entrypoint expose
```

## Clavier

- flèches / `hjkl` : naviguer entre les tuiles
- `Entrée` : basculer sur le pane sélectionné
- `1`-`9` : saut direct
- `Échap` / `q` : fermer

## Notes d'implémentation

- Plugin herdr v1 : manifeste + sous-processus. Le binaire parle directement au socket API (`HERDR_SOCKET_PATH`, JSON délimité par sauts de ligne, une requête par connexion) : `session.snapshot`, `pane.read`, `pane.zoom`. Pas de spawn de CLI : nécessaire pour le rafraîchissement live.
- Rafraîchissement live des tuiles à 4 Hz (aperçus, statuts d'agents, états de zoom). La structure sections/tuiles reste figée à l'ouverture : la sélection et la disposition spatiale ne bougent pas sous le curseur.
- Le focus par id n'existe pas dans l'API ; on utilise `pane zoom <id> --on|--off` avec le mode correspondant à l'état de zoom courant de la tab : no-op sur le zoom, mais `handle_pane_zoom` focalise le pane avant de vérifier le mode.
- Aperçus : `pane.read` source `recent_unwrapped`, format `ansi`.
- Rendu ANSI : parseur SGR maison (`src/ansi.rs`), suffisant car herdr régénère l'ANSI depuis sa grille de cellules (SGR pur, pas de séquences de curseur). Zéro dépendance de parsing.
- Grille : rétrécissement pour tout faire tenir, plancher 60×15, puis défilement (offset calculé sans état, la sélection reste visible). Un mode intermédiaire "tuiles dégradées en-tête seul" a été essayé puis retiré : il écrasait tout en laissant l'écran à moitié vide.

## À valider / limites connues

1. La fermeture du popup ne doit pas restaurer le focus antérieur par-dessus celui posé par le plugin (documenté pour `overlay`, non documenté pour `popup`) - test en session interactive requis.
2. Rafraîchissement séquentiel dans la boucle d'événements ; avec énormément de panes ou un serveur chargé, le tick de 250 ms pourrait s'étirer - à paralléliser si ça se sent.
3. Les tabs/panes créés ou fermés pendant que l'overlay est ouvert n'apparaissent/disparaissent pas (structure figée à l'ouverture, seuls contenus et statuts sont rafraîchis).
