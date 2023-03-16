faucet-cln:
	nigiri faucet cln 1

faucet-lspd-lnd:
	address=`docker-compose exec lspd-lnd lncli newaddress p2wkh | jq -r .address`; \
	nigiri faucet $$address 1

connect-cln-to-lspd-lnd:
	id=`docker-compose exec lspd-lnd lncli getinfo | jq -r .identity_pubkey`; \
	nigiri cln connect $$id@lspd-lnd:9739

open-channel-cln-to-lspd-lnd:
	id=`docker-compose exec lspd-lnd lncli getinfo | jq -r .identity_pubkey`; \
	nigiri cln fundchannel $$id 10000000 
	nigiri rpc -generate 6

list-channels-cln:
	nigiri cln listchannels

list-channels-lspd-lnd:
	docker-compose exec lspd-lnd lncli listchannels

wait-for-cln:
	./wait-for-cln.sh

setup-channel-cln-lspd-lnd: faucet-cln faucet-lspd-lnd connect-cln-to-lspd-lnd wait-for-cln open-channel-cln-to-lspd-lnd
