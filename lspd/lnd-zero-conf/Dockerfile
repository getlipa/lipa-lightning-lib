FROM golang:1.19

#RUN git clone --branch experiment --depth 1 https://github.com/danielgranhao/lnd
RUN git clone --branch breez-node-v0.15.4-no-anchor-zero-conf --depth 1 https://github.com/danielgranhao/lnd

RUN cd lnd \
    && go build -tags=signrpc,walletrpc,chainrpc,invoicesrpc,peersrpc ./cmd/lnd/ \
    && go build -tags=signrpc,walletrpc,chainrpc,invoicesrpc,peersrpc ./cmd/lncli/

COPY ./ /root/.lnd
COPY ./lncli /bin/

ENTRYPOINT ["/go/lnd/lnd"]
