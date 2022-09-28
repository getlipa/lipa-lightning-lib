NIGIRI_DATA ?= ~/.nigiri
LND_DATA = ${NIGIRI_DATA}/volumes/lnd

all: lnd.env

lnd.env: ${LND_DATA}/tls.cert ${LND_DATA}/data/chain/bitcoin/regtest/admin.macaroon
	rm --force $@
	@echo "Generating $@"
	@echo -n "LND_CERT=" > $@
	@sed --null-data 's/\n/\\n/g' ${LND_DATA}/tls.cert >> $@
	@echo "" >> $@
	@echo -n "LND_MACAROON_HEX=" >> $@
        # TODO: What type of macaroon should it be?
	@xxd -plain ${LND_DATA}/data/chain/bitcoin/regtest/admin.macaroon | tr --delete '\n' >> $@
	@echo "" >> $@

clean:
	rm --force lnd.env

.PHONY: all clean
