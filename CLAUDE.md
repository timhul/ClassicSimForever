# Introduction

This project contains ClassicSimForever, a simulator for World of Warcraft: Forever.
It is a C++ to Rust port from ClassicSim, a simulator for World of Warcraft: Classic.

# Main technical differences

ClassicSimForever uses Rust instead of C++ Qt.

The focus is initially on porting the engine itself, and focus on GUI later (if ever).

The data (spells, etc.) used to be authored by hand in the legacy C++ version. Now the data is
derived from the actual client side database tables. Not everything is present in these tables and
needs to be reimplemented.

There are also actual data differences due to the game versions (Forever vs Classic) being slightly
different.

# Development flow

(1) Planning phase

Create a plan for what to do, and write this plan to `TASKS.md`. If there is already
content in `TASKS.md` then append to the TASKS.md.

Do not implement anything during the planning phase.

(2) Development phase

Implement the given task in `TASKS.md`. Add tests if relevant. Make sure building works.
Run `cargo clippy` and other relevant code linters. Commit each substep (e.g. 4.1 is one commit).

Toolchain note: cargo/rustc are installed at `C:\Users\timhu\.cargo\bin` but are not on the shell
`PATH` in this environment; invoke via full path or add to PATH.
