# Introduction

This project contains ClassicSimForever, a simulator for World of Warcraft Forever.
It is a C++ to Rust port from ClassicSim, a simulator for World of Warcraft Classic.

# Main technical differences

ClassicSimForever does not use C++ Qt, and instead uses Rust.

The focus is initially on porting the engine itself, and focus on GUI later (if ever).

The data (spells, etc) handled by the simulator is different due to different game
versions being supported. The data should not be hardcoded, other than for prototyping.
There are too many spells and items for production-ready code to hardcode.

# Development flow

(1) Planning phase

Create a plan for what to do, and write this plan to `TASKS.md`. If there is already
content in `TASKS.md` then append to the TASKS.md.

Do not implement anything during the planning phase.

(2) Development phase

Implement the given task in `TASKS.md`. Add tests if relevant. Make sure building works.
Run `cargo clippy` and other relevant code linters.
