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

## Prerequisites

* [docker](https://docs.docker.com/get-docker/) with [docker-compose](https://docs.docker.com/compose/install/)
* [nigiri](https://nigiri.vulpem.com/)

## Getting started

### Configure the environments

Create `.cargo/config.toml` file with desired values (see `.cargo/config.toml.sample` as an example).

### Set up the local environment

This repository requires git submodules, to clone the project and the required submodules, run:
```sh
git clone --recursive git@github.com:getlipa/lipa-lightning-lib.git
```
To pull the submodules separately, run:
```sh
git submodule update --init
```

If running for the first time or after a change to the development environment, run:
```sh
make build-dev-env
```

To start a fresh development environment, run:
```sh
make start-dev-env
```
This will start:
* nigiri with 2 LN nodes (LND and CLN)
* an instance of [LSPD](https://github.com/breez/lspd) with an LND node
* an instance of [RGS server](https://github.com/lightningdevkit/rapid-gossip-sync-server)

### Clean the environment

Once finished, to stop these services, run:
```sh
make stop-dev-env
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
