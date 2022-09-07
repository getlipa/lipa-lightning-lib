# lipa-lightning-lib (3L)

> **Warning**
> This library is not production ready yet.

# Build

# Test

Start bitcoin core:
```sh
bitcoind -chain=regtest -rpcuser=polaruser -rpcpassword=polarpass
```

Start the example node:
```sh
cargo run --example node
```

View logs in `./.ldk/logs.txt`.
