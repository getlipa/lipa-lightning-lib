FROM golang:1.19.7

RUN git clone --depth 1 --branch lipa-node-v0.16.2-beta https://github.com/getlipa/lnd-lspd lnd

RUN cd lnd \
    && go build -tags=signrpc,walletrpc,chainrpc,invoicesrpc,peersrpc ./cmd/lnd/ \
    && go build -tags=signrpc,walletrpc,chainrpc,invoicesrpc,peersrpc ./cmd/lncli/

COPY ./ /root/.lnd
COPY ./lncli /bin/

ENTRYPOINT ["/go/lnd/lnd"]
