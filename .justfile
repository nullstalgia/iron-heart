# https://github.com/casey/just
set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

alias r := run
alias b := build
alias br := build-release
alias rr := release
alias rel := release
alias t := test
alias ti := test-integration
alias c := clippy
alias d := dummy
alias ws := websocket
alias wsd := websocket-dummy

run *ARGS:
  cargo run {{ARGS}}

build *ARGS:
  cargo build {{ARGS}}

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

ble:
  cargo run -- ble

dummy *ARGS:
  cargo run -- dummy {{ARGS}}

vhs:
  cargo run -- dummy --vhs -s 5.0

websocket *ARGS:
  cargo run -- ws {{ARGS}}

websocket-dummy *ARGS:
  cargo run --example websocket_dummy -- {{ARGS}}
