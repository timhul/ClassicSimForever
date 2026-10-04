
# ClassicSim

This repository contains a raid simulator for World of Warcraft: Forever.

## Try it in the browser

https://timhul.github.io/ClassicSimForever/live/ plays one iteration of the simulation as it
happens: pick a character setup, change its gear, play it by its rotation or from the keyboard,
and step through the casts.

## Running locally

Install `cargo`, the Rust package manager (google its instructions for your OS).

```
---------------------------------
# For a list of all available subcommands
cargo run --release -p csim-cli -- --help

# Arguments for the 'run' subcommand
cargo run --release -p csim-cli -- run --help

# Your standard single character, 10k iterations, random seed run
cargo run --release -p csim-cli -- run data/characters/warrior_fury_dw_orc.yaml
# To watch a formatted output, use --output-format and --output-file
cargo run --release -p csim-cli -- run data/characters/warrior_fury_dw_orc.yaml --output-format=html --output-file=out.html

# Sweep runs all (valid) permutations from a set of variation points (e.g. race)
# Terminal only output. This is intended for local use right now.
cargo run --release -p csim-cli -- sweep --help

# E.g. use a sweep for all races with a DW Fury setup
cargo run --release -p csim-cli -- sweep data/sweeps/dw_fury_races.yaml
```

## LLM usage

It is a Rust port of the C++ ClassicSim for 2019 Classic, which was developed between 2018-2020,
long before LLMs became prevalent. This project would not have been resurrected without the help of
LLMs.
