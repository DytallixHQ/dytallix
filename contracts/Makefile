fmt:
	cargo fmt

test:
	cargo test

test-examples:
	cargo test --manifest-path examples/counter/Cargo.toml
	cargo test --manifest-path examples/reward_splitter/Cargo.toml
	cargo test --manifest-path examples/algorithm_guard/Cargo.toml

check: fmt test test-examples

