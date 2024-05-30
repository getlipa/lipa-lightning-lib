use rocket::serde::Serialize;
use rocket::{futures::lock::Mutex, get, launch, post, routes, Config, State};
use rocket_dyn_templates::{context, Template};
use std::{collections::HashMap, net::Ipv4Addr};
use sha2::Sha256;
use sha2::Digest;

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "rocket::serde")]
struct Voucher {
	hash: String,
	amount_sats: u32,
}

type VoucherMap = Mutex<HashMap<String, Voucher>>;

#[get("/")]
fn index() -> Template {
    Template::render("index", context![])
}

#[post("/<hash>/<amount_sats>")]
async fn post_voucher(hash: String, amount_sats: u32, vouchers: &State<VoucherMap>) -> String {
	let voucher = Voucher {hash: hash.clone(), amount_sats};
	vouchers.lock().await.insert(hash, voucher.clone());
	format!("{voucher:?}")
}

#[get("/w?<lightning>")]
async fn resolve_voucher(lightning: &str, vouchers: &State<VoucherMap> ) -> Option<Template> {
	let preimage = lightning;
	let hash = sha256(preimage);
	println!("{preimage} hahes to {hash}");
	if let Some(voucher) = vouchers.lock().await.get(&hash) {
		return Some(Template::render("voucher", context![preimage, voucher]));
	}
	None
}

#[launch]
fn rocket() -> _ {
    let config = Config {
        port: 8000,
        address: Ipv4Addr::new(0, 0, 0, 0).into(),
        log_level: rocket::config::LogLevel::Normal,
        ..Config::debug_default()
    };
    rocket::custom(&config)
        .mount("/", routes![index, post_voucher, resolve_voucher])
        .manage(VoucherMap::default())
        .attach(Template::fairing())
}

fn sha256(data: &str) -> String {
	let mut hasher = Sha256::new();
    hasher.update(data);
	hex::encode(hasher.finalize())
}
