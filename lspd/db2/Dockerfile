FROM postgres:14.5-alpine

RUN apk add --no-cache git

WORKDIR /usr/src
RUN git clone https://www.github.com/breez/lspd
WORKDIR /usr/src/lspd
RUN git reset --hard 7511c1d46a8460931cb982c32b83d58e0eb90231
RUN mv postgresql/migrations /migrations
