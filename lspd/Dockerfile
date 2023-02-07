FROM golang:1.19-alpine as builder

WORKDIR /usr/src
COPY . lspd/
WORKDIR /usr/src/lspd
RUN go mod download
RUN go get github.com/breez/lspd
RUN go build -buildvcs=false .

FROM alpine as final

COPY --from=builder /usr/src/lspd/lspd /bin/

CMD ["lspd"]
