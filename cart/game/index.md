# cart/game — Interactive game cartridges

Games that showcase the LLM-as-CPU pattern: structured input, LLM reasoning, structured output. Zero hardware requirements, maximum didactic value. Each game is a separate cartridge, mounted independently by the kernel.

| Cartridge | Description |
|---|---|
| [`tetris`](tetris/full.md) | Turn-based Tetris. Methods: `move({action})`, `observe`, `reset`. The LLM plays by observing the board and choosing moves. Validates the kernel against a single-loop arcade game. |
| [`scavenger`](scavenger/full.md) | Grid-world quest. Methods: `move({dir})`, `pickup`, `drop`, `look`. The LLM finds a target object and delivers it to a destination, navigating around walls and pits. Validates the kernel against a multi-step planning game whose compiled-state shape mirrors `skillos_robot`'s SceneGraph — direct path from JS demo to real-world robot test. |
