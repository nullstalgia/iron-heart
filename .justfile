# https://github.com/casey/just
set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

alias r := run
alias b := build
alias br := build-release
alias rel := release
alias t := test
alias ti := test-integration
alias c := clippy
alias d := dummy
alias ws := websocket
alias wsd := websocket-dummy

run:
  cargo run

build:
  cargo build

release:
  cargo run --release --features portable

build-release:
  cargo build --release --features portable

test:
  cargo test

test-integration:
  cargo test -- --ignored --nocapture --test-threads=1

clippy:
  cargo clippy --all-targets --all-features

# Shortcuts for subcommands

dummy:
  cargo run -- dummy

websocket:
  cargo run -- ws

websocket-dummy:
  cargo run --example websocket_dummy
