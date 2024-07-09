use honeybadger::asynchronous::Auth;
use lazy_static::lazy_static;
use std::sync::Mutex;

const LIGHTNING_ADDRESS_STUB: &str = "static.mock@wallet.lipa.swiss";

pub async fn assign_lightning_address(_backend_url: &str, _auth: &Auth) -> graphql::Result<String> {
    Ok(LIGHTNING_ADDRESS_STUB.to_string())
}

pub async fn submit_lnurl_pay_invoice(
    _backend_url: &str,
    _auth: &Auth,
    _id: String,
    _invoice: Option<String>,
) -> graphql::Result<()> {
    Ok(())
}

pub async fn request_phone_number_verification(
    _backend_url: &str,
    _auth: &Auth,
    _number: String,
    encrypted_number: String,
) -> graphql::Result<()> {
    let mut phone_number = PHONE_NUMBER.lock().unwrap();
    *phone_number = Some(encrypted_number);
    Ok(())
}

pub async fn verify_phone_number(
    _backend_url: &str,
    _auth: &Auth,
    _number: String,
    _otp: String,
) -> graphql::Result<()> {
    Ok(())
}

lazy_static! {
    static ref PHONE_NUMBER: Mutex<Option<String>> = Mutex::new(None);
}

pub async fn query_verified_phone_number(
    _backend_url: &str,
    _auth: &Auth,
) -> graphql::Result<Option<String>> {
    Ok(PHONE_NUMBER.lock().unwrap().clone())
}
