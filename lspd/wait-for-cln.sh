#!/bin/sh

i=0
while [ $i -lt 40 ]; do
    cln_height=`nigiri cln getinfo | jq .blockheight`
    bitcoin_height=`nigiri rpc getblockcount`
    if [ $cln_height -eq $bitcoin_height ]; then
	echo "done"
	exit 0
    fi
    echo "$i: CLN is syncing $cln_height out of $bitcoin_height ..."
    i=$((i+1))
    sleep 1
done
echo "CLN is not synced"
exit 1
