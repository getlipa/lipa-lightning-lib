use rocket::serde::Serialize;
use rocket::{futures::lock::Mutex, get, launch, post, routes, Config, State};
use rocket_dyn_templates::{context, Template};
use sha2::Digest;
use sha2::Sha256;
use std::{collections::HashMap, net::Ipv4Addr};

const DOMAIN: &str = "https://voucher.zzd.es";

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "rocket::serde")]
struct Voucher {
    hash: String,
    amount_sats: u32,
    seal_required: bool,
}

type VoucherMap = Mutex<HashMap<String, Voucher>>;

#[get("/")]
fn index() -> Template {
    Template::render("index", context![])
}

#[post("/<hash>/<amount_sats>")]
async fn post_voucher(hash: String, amount_sats: u32, vouchers: &State<VoucherMap>) -> String {
    const DOMAIN: &str = "https://voucher.zzd.es";
    let voucher = Voucher {
        hash: hash.clone(),
        amount_sats,
        seal_required: false,
    };
    println!("New voucher registered: {voucher:?}");
    //    let lnurl_raw = format!("{DOMAIN}/lnurl/{hash}");
    //    let hrp = bech32::Hrp::parse("lnurl").expect("valid hrp");
    //    let lnurl =
    //        bech32::encode::<bech32::Bech32>(hrp, lnurl_raw.as_bytes()).expect("bech32 encoding");
    vouchers.lock().await.insert(hash, voucher);

    json::object! {
        lnurl_prefix: format!("{DOMAIN}/lnurl/"),
        url_prefix: format!("{DOMAIN}?lightning="),
    }
    .to_string()
}

#[get("/lnurl/<preimage>")]
async fn lnurl(preimage: String, vouchers: &State<VoucherMap>) -> Option<String> {
    let hash = sha256(&preimage);
    let hash = dbg!(hash);
    if let Some(voucher) = vouchers.lock().await.get(&hash) {
        let json = json::object! {
            tag: "withdrawRequest",
            callback: format!("{DOMAIN}/lnurl"),
            maxSendable: voucher.amount_sats,
            minSendable: voucher.amount_sats,
            k1: preimage,
            seal_required: voucher.seal_required,
        };
        return Some(json.to_string());
    }
    None
}

#[get("/lnurl?<k1>&<pr>")]
async fn submit_lnurl(k1: String, pr: String, vouchers: &State<VoucherMap>) -> Option<String> {
    let preimage = k1;
    let hash = sha256(&preimage);
    let hash = dbg!(hash);
    if let Some(_voucher) = vouchers.lock().await.get(&hash) {
        // TODO: Validate that invoice for the right amount.
        println!("Redeem voucher {preimage} to {pr}");
        return Some(json::object! {status: "OK"}.to_string());
    }
    None
}

#[get("/?<lightning>")]
async fn resolve_voucher(lightning: &str, vouchers: &State<VoucherMap>) -> Option<Template> {
    let (_, bytes) = bech32::decode(lightning).expect("Invalid lnurl");
    let url = String::from_utf8(bytes).expect("Invalid lnurl");
    let url = dbg!(url);
    let (_url, preimage) = url.rsplit_once('/').expect("Missing / in url");
    let preimage = dbg!(preimage);
    let hash = sha256(preimage);
    let hash = dbg!(hash);
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
        .mount(
            "/",
            routes![index, post_voucher, resolve_voucher, lnurl, submit_lnurl],
        )
        .manage(VoucherMap::default())
        .attach(Template::fairing())
}

fn sha256(data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}
