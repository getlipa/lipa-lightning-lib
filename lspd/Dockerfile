FROM golang:1.19

WORKDIR /usr/src
COPY . lspd/
WORKDIR /usr/src/lspd
RUN go mod download
RUN go get github.com/breez/lspd
RUN go build -buildvcs=false .

CMD ["./lspd"]
