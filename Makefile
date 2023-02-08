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
test: TEST = ''
test:
	cargo test --lib --verbose -- $(TEST)
	cargo test --manifest-path eel/Cargo.toml --lib --verbose -- $(TEST)

.PHONY: integrationtests
integrationtests: FILE = *
integrationtests: TEST = ''
integrationtests:
	cargo test --manifest-path eel/Cargo.toml --features nigiri --test '$(FILE)' -- $(TEST)

.PHONY: testall
testall: test integrationtests

.PHONY: fmt
fmt:
	cargo fmt
	cargo fmt --manifest-path eel/Cargo.toml

.PHONY: fmt-check
fmt-check:
	cargo fmt -- --check
	cargo fmt --manifest-path eel/Cargo.toml -- --check

.PHONY: clippy
clippy:
	cargo clippy -- -D warnings
	cargo clippy --manifest-path eel/Cargo.toml -- -D warnings

.PHONY: udeps
udeps:
	cargo +nightly udeps
	cargo +nightly udeps --manifest-path eel/Cargo.toml

# Check that we stick to `mod tests {` style.
.PHONY: check-mod-test
check-mod-test:
	! grep --recursive --include="*.rs" "mod test " *

# Quick tests to run before creating a PR.
.PHONY: pr
pr: fmt buildall test clippy check-mod-test

.PHONY: runnode
runnode:
	cargo run --manifest-path eel/Cargo.toml --example node

.PHONY: build-dev-env
build-dev-env:
	docker-compose -f lspd/compose.yaml build
	docker-compose -f rgs/compose.yaml build

.PHONY: start-dev-env
start-dev-env:
	nigiri start --ln
	docker-compose -f lspd/compose.yaml up -d
	docker-compose -f rgs/compose.yaml up -d

.PHONY: connect-rgs-ln-node
connect-rgs-ln-node:
	id=`docker-compose -f rgs/compose.yaml exec rgs-cln lightning-cli --network=regtest getinfo | jq -r .id`; \
	nigiri cln connect $$id@rgs-cln:9937

.PHONY: stop-dev-env
stop-dev-env:
	docker-compose -f rgs/compose.yaml down
	docker-compose -f lspd/compose.yaml down
	nigiri stop --delete
