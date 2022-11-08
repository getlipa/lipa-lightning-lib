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

Start LSPD + Custom LND
 1. Go to `./lspd`
 3. Run `docker-compose up lspd` to start LSPD and the LSP node.

## Step 2: Fire it up!
Start the example node:
```sh
cargo run --example node
```

The example node will connect to LSPD and get information about lightning node
pubkey and fees.

### Logs
View logs in `./.ldk/logs.txt`.
