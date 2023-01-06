FROM golang:1.19 as builder

WORKDIR /usr/src
RUN git clone https://www.github.com/breez/lspd
WORKDIR /usr/src/lspd
RUN git reset --hard 7511c1d46a8460931cb982c32b83d58e0eb90231
RUN CGO_ENABLED=0 go build -buildvcs=false -a -installsuffix cgo -o .

FROM elementsproject/lightningd:latest

COPY ./hsm_secret /root/.lightning/regtest/hsm_secret
COPY --from=builder /usr/src/lspd/lspd /lspd

ENTRYPOINT ["/usr/bin/tini", "-g", "--", "./entrypoint.sh"]
