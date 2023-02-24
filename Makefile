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
	cargo test --workspace --features nigiri --test '$(FILE)' -- $(TEST)

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

# Quick tests to run before creating a PR.
.PHONY: pr
pr: fmt buildall test clippy check-mod-test

.PHONY: run-eel
run-eel:
	cargo run --manifest-path eel/Cargo.toml --example node

.PHONY: run-3l
run-3l: ARGS =
run-3l:
	cargo run --example node -- $(ARGS)

.PHONY: build-dev-env
build-dev-env:
	docker-compose -f lspd/compose.yaml build
	docker-compose -f rgs/compose.yaml build

.PHONY: start-dev-env
start-dev-env:
	nigiri start --ln
	docker-compose -f lspd/compose.yaml up -d
	docker-compose -f rgs/compose.yaml up -d
	$(MAKE) connect-rgs-ln-node

.PHONY: connect-rgs-ln-node
connect-rgs-ln-node:
	id=`docker-compose -f rgs/compose.yaml exec rgs-cln lightning-cli --network=regtest getinfo | jq -r .id`; \
	nigiri cln connect $$id@rgs-cln:9937

.PHONY: stop-dev-env
stop-dev-env:
	docker-compose -f rgs/compose.yaml down
	docker-compose -f lspd/compose.yaml down
	nigiri stop --delete
