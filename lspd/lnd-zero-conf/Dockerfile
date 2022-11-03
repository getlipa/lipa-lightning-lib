FROM alpine:latest AS builder
RUN apk update
RUN apk add git go musl-dev make bash
RUN git clone https://github.com/danielgranhao/lnd

RUN cd lnd \
    && go build ./cmd/lnd/ \
    && go build ./cmd/lncli/

VOLUME /root/.lnd
EXPOSE 10013 9739

COPY ./lnd root/.lnd
COPY ./lnd-zero-conf/lnd.conf /lnd.conf
COPY ./lnd-zero-conf/start.sh .
RUN chmod +x ./start.sh
ENTRYPOINT /start.sh