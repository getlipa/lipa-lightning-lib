.PHONY: check
check:
	cargo check

.PHONY: checkall
checkall:
	cargo check --all-targets --all-features

.PHONY: build
build:
	cargo build

.PHONY: buildall
buildall:
	cargo build --all-targets --all-features

.PHONY: test
TEST = *
test:
	cargo test --lib -- $(TEST)

.PHONY: integrationtests
TEST = *
integrationtests:
	cargo test --features nigiri --test '$(TEST)' -- --test-threads 1

.PHONY: testall
testall: test integrationtests

.PHONY: fmt
fmt:
	cargo fmt

.PHONY: clippy
clippy:
	cargo clippy

# Quick tests to run before creating a PR.
.PHONY: pr
pr: fmt buildall test clippy

.PHONY: runnode
runnode:
	cargo run --example node
