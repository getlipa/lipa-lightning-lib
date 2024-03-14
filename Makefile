.PHONY: check
check:
	cargo check --workspace

.PHONY: checkall
checkall:
	cargo check --workspace --all-targets

.PHONY: build
build:
	cargo build --workspace

.PHONY: buildall
buildall:
	cargo build --workspace --all-targets

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
	cargo clippy --all --tests --examples -- -D warnings

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

.PHONY: bump-wild
bump-wild:
	@newest_tag=$$(curl -s "https://api.github.com/repos/getlipa/wild/tags" | jq -r '.[0].name'); \
	cargo_toml_files=$$(echo './Cargo.toml'); \
	echo "$$cargo_toml_files" | xargs sed -i "s/\(git = \"https:\/\/github.com\/getlipa\/wild\",\).*\(tag = \"[^\"]*\"\)/\1 tag = \"$$newest_tag\"/g"; \
    echo "Bumped wild to $$newest_tag"; \

.PHONY: run-node
run-node: ARGS =
run-node:
	cargo run --example node -- $(ARGS)

.PHONY: run-parser-demo
run-parser-demo:
	cargo run --package parser --example demo

.PHONY: run-notification-handler
run-notification-handler: ARGS =
run-notification-handler:
	cargo run --example notification_handler -- $(ARGS)
