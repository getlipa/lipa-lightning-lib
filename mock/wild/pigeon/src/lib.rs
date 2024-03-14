use honey_badger::asynchronous::Auth;

const LIGHTNING_ADDRESS_STUB: &str = "static.mock@wallet.lipa.swiss";

pub async fn assign_lightning_address(_backend_url: &str, _auth: &Auth) -> graphql::Result<String> {
    Ok(LIGHTNING_ADDRESS_STUB.to_string())
}
