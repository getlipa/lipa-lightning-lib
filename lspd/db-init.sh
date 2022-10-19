#!/usr/bin/env sh

for sql_file in /migrations/*.up.sql; do
    echo "Applying $sql_file"
    psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" < $sql_file
done
