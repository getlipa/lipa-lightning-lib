# lipa-lightning-lib (3L)

> [!IMPORTANT]
> The library is in beta.

A Rust library that implements the main lightning logic of the lipa wallet app.

***

# Build

## Prerequisites
* [protobuf](https://grpc.io/docs/protoc-installation/)

***
# Development environment

## Getting started

### Configure the environments

Create `.cargo/config.toml` file with desired values (see `.cargo/config.toml.sample` as an example).

***
# Example 3L node

## Setup

Create a new node:
```sh
make testregisternode
```

Copy the mnemonic and put it into `./.cargo/config.toml` under `BREEZ_SDK_MNEMONIC`.

> [!WARNING]
> Keep the mnemonic secure and safe, it is your local mainnet wallet.

## Run

To start the example node included in this repository, run:
```sh
make run-node
```

To start the example node in another environment:
```sh
make run-node ARGS=dev
```
`local` (default), `dev`, `stage`, and `prod` environments are available.

To test background receive:
 - start the regular example node, issue an invoice, and shut it down
 - run `make run-notification-handler ARGS=<payment hash of issued invoice>`
 - do either
   - don't pay the invoice or pay a different invoice → after the timeout of 60 secs, the action `None` should be printed
   - pay the invoice issued in step 1 → the action `ShowNotification` should be printed

#### Logs
View logs in `.3l_node_{ENVIRONMENT_CODE}/logs.txt`.

#### Reset node
To start a fresh node, delete the `.3l_node_{ENVIRONMENT_CODE}` directory.

***
# Interface documentation
The rust interface of the latest released version is documented [here](https://getlipa.github.io/lipa-lightning-lib/uniffi_lipalightninglib/).

For the language-specific calls, refer to the respective language bindings:
 - [Kotlin](https://github.com/getlipa/lipa-lightning-lib-android)
 - [Swift](https://github.com/getlipa/lipa-lightning-lib-swift)
