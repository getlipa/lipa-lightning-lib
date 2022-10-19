# lipa-lightning-lib (3L)

> **Warning**
> This library is not production ready yet.

# Build

 - install protobuf

# Test locally with Nigiri

## Step 1: Setup Environment
Start [Nigiri Bitcoin](https://github.com/vulpemventures/nigiri)
at least **v0.4.4**:
```sh
nigiri start --ln
```

Start LSPD
 1. Go to `./lspd`
 2. Run `make` to generate `lnd.env` file with LND TLS certificate and macaroons
 3. Run `docker-compose up lspd` to start LSPD.
 Mind that `lspd` container depends on `db` container which takes time to start.

Note: make assumes that nigiri data is in `~/.nigiri`,
but you can customize it by `NIGIRI_DATA=./nigiri-data make clean all`.

## Step 2: Fire it up!
Start the example node:
```sh
cargo run --example node
```

The example node will connect to LSPD and get information about lightning node
pubkey and fees.

### Logs
View logs in `./.ldk/logs.txt`.
