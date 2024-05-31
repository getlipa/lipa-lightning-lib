FROM debian:bookworm-slim

COPY voucherserver/templates/ templates/
COPY target/debug/voucherserver .

CMD ["./voucherserver"]
