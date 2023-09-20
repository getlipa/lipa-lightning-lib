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

### Set up the local environment

This repository requires git submodules, to clone the project and the required submodules, run:
```sh
git clone --recursive git@github.com:getlipa/lipa-lightning-lib.git
```

***
# Example 3L node

To start the example node included in this repository, run:
```sh
make run-3l
```

To start the example node in another environment:
```sh
make run-3l ARGS=dev
```
`local` (default), `dev`, `stage`, and `prod` environments are available.

#### Logs
View logs in `.3l_node_{ENVIRONMENT_CODE}/logs.txt`.

#### Reset node
To start a fresh node, delete the `.3l_node_{ENVIRONMENT_CODE}` directory.

***
# Interface documentation
The consumer interface is most aptly documented in the interface file
[`src/lipalightninglib.udl`](src/lipalightninglib.udl).
For the language-specific calls, refer to the respective language bindings:
 - [Kotlin](https://github.com/getlipa/lipa-lightning-lib-android)
 - [Swift](https://github.com/getlipa/lipa-lightning-lib-swift)
