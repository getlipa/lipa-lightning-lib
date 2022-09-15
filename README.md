# lipa-lightning-lib (3L)

> **Warning**
> This library is not production ready yet.

# Build

# Test

Start [Nigiri Bitcoin](https://github.com/vulpemventures/nigiri):
```sh
nigiri start --ln
```
*The `--ln` flag is not strictly required, but it starts an LND and a CLN node
which you are likely going to use to test the library.*

Start the example node:
```sh
cargo run --example node
```

View logs in `./.ldk/logs.txt`.
