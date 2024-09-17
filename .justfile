# https://github.com/casey/just
set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

run:
  cargo run

alias rel := release

release:
  cargo run --release --features portable

alias t := test

test:
  cargo test

alias ti := test-integration

test-integration:
  cargo test -- --ignored --nocapture --test-threads=1

alias c := clippy

clippy:
  cargo clippy --all-targets --all-features
