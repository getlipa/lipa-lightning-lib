FROM golang:1.19

RUN git clone --branch v0.15.4-beta --depth 1 https://github.com/danielgranhao/lnd

RUN cd lnd \
    && go build -tags=signrpc,walletrpc,chainrpc,invoicesrpc,peersrpc ./cmd/lnd/ \
    && go build -tags=signrpc,walletrpc,chainrpc,invoicesrpc,peersrpc ./cmd/lncli/

COPY ./ /root/.lnd
COPY ./lncli /bin/

ENTRYPOINT ["/go/lnd/lnd"]
