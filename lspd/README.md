# Setup

Make sure to use nigiri at least **v0.4.4**, it contains an important fix from Andrei.
Otherwise you risk to get:
```sh
Failed to dial target host "lnd:10009": x509: certificate is valid for 3a465453271a, localhost, unix, unixpacket, bufconn, not lnd
```

# Run
 - go to `./lspd` directory
 - run `nigiri start --ln`
 - run `make` to generate `lnd.env` file with LND TLS certificate and macaroons
 - run `docker-compose up lspd` to start LSPD. Mind that `lspd` container depends on `db` container which takes time to start.

Note: make assumes that nigiri data is in `~/.nigiri`,
but you can customize it by `NIGIRI_DATA=./nigiri-data make clean all`.

# Test
Use [grpcurl](https://github.com/fullstorydev/grpcurl) for testing.

```sh
grpcurl -plaintext -proto lspd.proto \
  -rpc-header "authorization: Bearer iQUvOsdk4ognKshZB/CKN2vScksLhW8i13vTO+8SPvcyWJ+fHi8OLgUEvW1N3k2l" \
  -d '{"pubkey": "1234"}' \
  localhost:6666 \
  lspd.ChannelOpener/ChannelInformation

# Defint your node pub key.
CLIENT_NODE_PUBKEY=xxx

# Request LND to open a channel to your node. Note you need to connect first.
grpcurl -plaintext -proto lspd.proto \
  -rpc-header "authorization: Bearer iQUvOsdk4ognKshZB/CKN2vScksLhW8i13vTO+8SPvcyWJ+fHi8OLgUEvW1N3k2l" \
  -d '{"pubkey": "$CLIENT_NODE_PUBKEY"}' \
  localhost:6666 \
  lspd.ChannelOpener/OpenChannel
```
