# lipa-lightning-lib (3L)

> **Warning**
> This library is not production ready yet.

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

To start the example node included in this repository, run:
```sh
make run-node
```

To start the example node in another environment:
```sh
make run-node ARGS=dev
```
`local` (default), `dev`, `stage`, and `prod` environments are available.

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
