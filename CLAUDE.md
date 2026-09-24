# Introduction

This project contains ClassicSimForever, a simulator for World of Warcraft: Forever.
It is a C++ to Rust port from ClassicSim, a simulator for World of Warcraft: Classic.

# Main technical differences

ClassicSimForever uses Rust instead of C++ Qt. No GUI for now.

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

Implement the given task in `TASKS.md`. Add tests if relevant.

Run `cargo clippy`, `cargo nextest run` to verify building works and source code passes linting.

Commit each defined task once complete.

Toolchain note: cargo/rustc are installed at `C:\Users\timhu\.cargo\bin` but are not on the shell
`PATH` in this environment; invoke via full path or add to PATH.

# Running the sim

`cargo run --release -p csim-cli -- run data/characters/dw_fury_orc.yaml --iterations 10000`
