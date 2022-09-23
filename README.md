# lipa-lightning-lib (3L)

> **Warning**
> This library is not production ready yet.

# Build

# Test locally with Nigiri
### Step 1: Environment
Start [Nigiri Bitcoin](https://github.com/vulpemventures/nigiri):
```sh
nigiri start --ln
```
*The `--ln` flag is not strictly required, but it starts an LND and a CLN node
which you are likely going to use to test the library.*

### Step 2: Configuration
Create a .env file:
```sh
cp examples/node/.env.example examples/node/.env
```

Then, configure a Nigiri Lightning node as your LSP.

The get the port for the `LSP_NODE_ADDRESS` it helps to look at the output your `nigiri start` command.

To get the `LSP_NODE_PUBKEY` you can run the following command for example:
```sh
nigiri lnd getinfo | jq -r .identity_pubkey
```

### Step 3: Fire it up!
Start the example node:
```sh
cargo run --example node
```

### Logs
View logs in `./.ldk/logs.txt`.
