FROM golang:1.19

RUN git clone --depth 1 https://github.com/danielgranhao/lnd

RUN cd lnd \
    && go build ./cmd/lnd/ \
    && go build ./cmd/lncli/

COPY ./ /root/.lnd

ENTRYPOINT ["/go/lnd/lnd"]
