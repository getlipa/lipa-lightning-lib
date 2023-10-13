.PHONY: check
check:
	cargo check --workspace

.PHONY: checkall
checkall:
	cargo check --workspace --all-targets --all-features

.PHONY: build
build:
	cargo build --workspace

.PHONY: buildall
buildall:
	cargo build --workspace --all-targets --all-features

.PHONY: clean
clean:
	cargo clean

.PHONY: test
test: TEST = ''
test:
	cargo test --workspace --lib --verbose -- $(TEST)

.PHONY: integrationtests
integrationtests: FILE = *
integrationtests: TEST = ''
integrationtests:
	cargo test --workspace --test '$(FILE)' -- $(TEST)

.PHONY: testregisternode
testregisternode:
	cargo test --test register_node_test -- --ignored --nocapture

.PHONY: testall
testall: test integrationtests

.PHONY: fmt
fmt:
	cargo fmt --all

.PHONY: fmt-check
fmt-check:
	cargo fmt --all -- --check

.PHONY: clippy
clippy:
	cargo clippy -- -D warnings

.PHONY: udeps
udeps:
	cargo +nightly udeps

# Check that we stick to `mod tests {` style.
.PHONY: check-mod-test
check-mod-test:
	! grep --recursive --include="*.rs" "mod test " *

.PHONY: check-udl
check-udl:
	! grep $$'\t' src/lipalightninglib.udl

.PHONY: doc
doc:
	cargo doc --no-deps

# Quick tests to run before creating a PR.
.PHONY: pr
pr: fmt buildall test clippy check-mod-test check-udl doc

.PHONY: run-3l
run-3l: ARGS =
run-3l:
	cargo run --example 3l-node -- $(ARGS)
