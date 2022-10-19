NIGIRI_DATA ?= ~/.nigiri
LND_DATA = ${NIGIRI_DATA}/volumes/lnd

all: lnd.env

lnd.env: ${LND_DATA}/tls.cert ${LND_DATA}/data/chain/bitcoin/regtest/admin.macaroon
	@echo "Generating $@"
        # Explicitly using /bin/echo to deal with `-n`, otherwise it does not work on macOS
        # https://stackoverflow.com/questions/11675070/makefile-echo-n-not-working
	@/bin/echo -n "LND_CERT=" > $@
	@sed 's/$$/\\n/g' ${LND_DATA}/tls.cert | tr -d '\n' >> $@
	@echo "" >> $@
	@/bin/echo -n "LND_MACAROON_HEX=" >> $@
        # TODO: What type of macaroon should it be?
	@xxd -plain ${LND_DATA}/data/chain/bitcoin/regtest/admin.macaroon | tr -d '\n' >> $@
	@echo "" >> $@

clean:
	rm -f lnd.env

.PHONY: all clean
