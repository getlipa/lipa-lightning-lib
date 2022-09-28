FROM golang:1.19

WORKDIR /usr/src
RUN git clone --quiet --depth 1 https://github.com/breez/lspd.git
WORKDIR /usr/src/lspd
RUN go mod download
RUN go get github.com/breez/lspd
RUN go build .
RUN ls

CMD ["./lspd"]
