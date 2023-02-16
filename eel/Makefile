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
	cargo test --lib -- $(TEST)

.PHONY: integrationtests
integrationtests: FILE = *
integrationtests: TEST = ''
integrationtests:
	LSPD_HOME=../lspd RGS_HOME=../rgs cargo test --features nigiri --test '$(FILE)' -- $(TEST)

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

.PHONY: build-dev-env
build-dev-env:
	docker-compose -f ../lspd/compose.yaml build
	docker-compose -f ../rgs/compose.yaml build

.PHONY: start-dev-env
start-dev-env:
	nigiri start --ln
	docker-compose -f ../lspd/compose.yaml up -d
	docker-compose -f ../rgs/compose.yaml up -d

.PHONY: stop-dev-env
stop-dev-env:
	docker-compose -f ../rgs/compose.yaml down
	docker-compose -f ../lspd/compose.yaml down
	nigiri stop --delete
