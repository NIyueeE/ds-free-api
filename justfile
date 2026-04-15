# Justfile for ai-free-api

# Run all checks: type check, lint, format, audit, unused deps
# 前置: cargo install cargo-audit && cargo install cargo-machete
check:
  cargo check
  cargo clippy -- -D warnings
  cargo fmt --check
  cargo audit
  cargo machete

# Run ds_core_cli example
ds-core-cli *ARGS:
  cargo run --example ds_core_cli -- {{ARGS}}

# Run openai_adapter/request submodule tests
test-adapter-request *ARGS:
  cargo test openai_adapter::request -- {{ARGS}}

# Run openai_adapter/response submodule tests
test-adapter-response *ARGS:
  cargo test openai_adapter::response -- {{ARGS}}

# Run openai_adapter_cli example
openai-adapter-cli *ARGS:
  cargo run --example openai_adapter_cli -- {{ARGS}}

# Run HTTP server
serve *ARGS:
  cargo run -- {{ARGS}}

# Run Python e2e tests (requires server running; will skip with hint if not)
e2e:
  cd py-e2e-tests && uv run python -m pytest

# Start server with e2e test config
e2e-serve:
  cargo run -- -c py-e2e-tests/config.toml
