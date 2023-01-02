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
* [nigiri](https://nigiri.vulpem.com/) (at least **v0.4.4**)

## Getting started

### Set up the environment

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

To configure a 3L node to use this environment, the following config should be used:
* network: Regtest
* esplora_api_url: http://localhost:30000
* lsp_node: NodeAddress{ pub_key: "\<the pubkey that was printed when starting the dev env>", host: "127.0.0.1:9739"}
* rgs_url: http://localhost:8080/snapshot/

### Clean the environment

Once finished, to stop these services, run:
```sh
make stop-dev-env
```

***
# Example 3L node

To start the example node included in this repository, run:
```sh
make runnode
```

#### Logs
View logs in `./.ldk/logs.txt`.

#### Reset node
To start a fresh node, delete the `./.ldk` directory.

***
# Interface documentation
The consumer interface is most aptly documented in the interface file `src/lipalightninglib.udl`.
For the language-specific calls, refer to the respective language bindings:
 - [Kotlin](https://github.com/getlipa/lipa-lightning-lib-android)
 - [Swift](https://github.com/getlipa/lipa-lightning-lib-swift)
