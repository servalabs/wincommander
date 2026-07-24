use serde::{Deserialize, Serialize};

const STORE_SECTION: &str = "pending_purchase";

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseInput {
    pub sku: String,
    #[serde(default)]
    pub seats: Option<u16>,
    pub email: String,
    pub phone: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingPurchase {
    idempotency_key: String,
    input: PurchaseInput,
    purchase_id: Option<String>,
    recovery_token: Option<String>,
    checkout_url: Option<String>,
    amount: Option<u64>,
    currency: Option<String>,
    expires_at: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatePurchaseResponse {
    ok: bool,
    purchase_id: Option<String>,
    recovery_token: Option<String>,
    checkout_url: Option<String>,
    amount: Option<u64>,
    currency: Option<String>,
    expires_at: Option<u64>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseView {
    pub purchase_id: Option<String>,
    pub sku: String,
    pub seats: Option<u16>,
    pub checkout_url: Option<String>,
    pub amount: Option<u64>,
    pub currency: Option<String>,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogOffer {
    pub sku: String,
    pub name: String,
    pub price_label: String,
    pub detail: String,
    pub device_rule: String,
    pub checkout_eligible: bool,
    pub min_seats: Option<u16>,
    pub max_seats: Option<u16>,
    pub seat_pricing_label: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogResponse {
    ok: bool,
    offers: Option<Vec<CatalogOffer>>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    ok: bool,
    state: Option<String>,
    provider_status: Option<String>,
    license_key: Option<String>,
    amount: Option<u64>,
    currency: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseStatus {
    pub state: String,
    pub provider_status: Option<String>,
    pub amount: Option<u64>,
    pub currency: Option<String>,
    pub license_key: Option<String>,
    pub activated: bool,
    pub activation_error: Option<String>,
}

fn load_pending() -> Result<Option<PendingPurchase>, String> {
    let value = crate::datastore::load(STORE_SECTION)?;
    if value.get("idempotencyKey").is_none() {
        return Ok(None);
    }
    serde_json::from_value(value)
        .map(Some)
        .map_err(|_| "Pending purchase state is invalid.".to_string())
}

fn save_pending(pending: &PendingPurchase) -> Result<(), String> {
    let value = serde_json::to_value(pending)
        .map_err(|_| "Failed to protect pending purchase state.".to_string())?;
    crate::datastore::save(STORE_SECTION, &value)
}

fn view(pending: PendingPurchase) -> Result<PurchaseView, String> {
    if let Some(url) = pending.checkout_url.as_deref() {
        if !checkout_url_is_trusted(url, &crate::license::license_api_base()?) {
            return Err(
                "Saved checkout link is no longer trusted. Start a new checkout.".to_string(),
            );
        }
    }
    Ok(PurchaseView {
        purchase_id: pending.purchase_id,
        sku: pending.input.sku,
        seats: pending.input.seats,
        checkout_url: pending.checkout_url,
        amount: pending.amount,
        currency: pending.currency,
        expires_at: pending.expires_at,
    })
}

async fn parse_json<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, String> {
    response
        .json::<T>()
        .await
        .map_err(|_| "The purchase service returned an invalid response.".to_string())
}

fn validate_purchase_input(input: &PurchaseInput) -> Result<(), String> {
    match input.sku.as_str() {
        // Personal offers use three transferable active activation/update/service
        // slots. The service owns the allowance, so a client can't alter it.
        "pro_lifetime" | "pro_membership" | "investigator" if input.seats.is_none() => {}
        "fleet" if matches!(input.seats, Some(1..=50)) => {}
        "pro_lifetime" | "pro_membership" | "investigator" => {
            return Err(
                "Personal offers include three transferable active device slots and do not use seats."
                    .to_string(),
            );
        }
        "fleet" => {
            return Err(
                "Choose 1-50 Fleet devices. Contact sales for more than 50 devices.".to_string(),
            );
        }
        _ => return Err("Unknown product.".to_string()),
    }
    let email = input.email.trim();
    if email.len() > 254 || !email.contains('@') {
        return Err("Enter a valid email address.".to_string());
    }
    Ok(())
}

fn checkout_url_is_trusted(candidate: &str, api_base: &str) -> bool {
    let Ok(checkout) = reqwest::Url::parse(candidate) else {
        return false;
    };
    let Ok(trusted) = reqwest::Url::parse(api_base) else {
        return false;
    };
    checkout.scheme() == "https"
        && trusted.scheme() == "https"
        && checkout.host_str() == trusted.host_str()
        && checkout.port_or_known_default() == trusted.port_or_known_default()
}

fn validate_catalog_offer(offer: &CatalogOffer) -> bool {
    let valid_sku = matches!(
        offer.sku.as_str(),
        "pro_lifetime" | "pro_membership" | "investigator" | "fleet"
    );
    let has_copy = !offer.name.trim().is_empty()
        && !offer.price_label.trim().is_empty()
        && !offer.detail.trim().is_empty()
        && !offer.device_rule.trim().is_empty();
    let seat_bounds_are_valid = match (offer.min_seats, offer.max_seats) {
        (Some(min), Some(max)) => min >= 1 && min <= max,
        (None, None) => true,
        _ => false,
    };
    valid_sku && has_copy && seat_bounds_are_valid
}

#[tauri::command]
pub async fn get_purchase_catalog() -> Result<Vec<CatalogOffer>, String> {
    let api_base = crate::license::license_api_base()?;
    let client = crate::net::doh_http_client()?;
    let response = client
        .get(format!("{}/v1/catalog", api_base))
        .send()
        .await
        .map_err(|error| format!("Couldn't reach the current offers service: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Current offers service returned HTTP {}.",
            response.status().as_u16()
        ));
    }
    let body: CatalogResponse = parse_json(response).await?;
    if !body.ok {
        return Err(body
            .error
            .unwrap_or_else(|| "Current offers are unavailable.".to_string()));
    }
    let offers = body
        .offers
        .ok_or_else(|| "Current offers response was incomplete.".to_string())?;
    if offers.len() != 4 || offers.iter().any(|offer| !validate_catalog_offer(offer)) {
        return Err("Current offers response was invalid.".to_string());
    }
    Ok(offers)
}

#[tauri::command]
pub async fn create_purchase(input: PurchaseInput) -> Result<PurchaseView, String> {
    validate_purchase_input(&input)?;

    let pending = match load_pending()? {
        Some(existing)
            if existing.input == input
                && existing.purchase_id.is_none()
                && existing.recovery_token.is_none() =>
        {
            existing
        }
        _ => PendingPurchase {
            idempotency_key: uuid::Uuid::new_v4().to_string(),
            input: input.clone(),
            purchase_id: None,
            recovery_token: None,
            checkout_url: None,
            amount: None,
            currency: None,
            expires_at: None,
        },
    };
    save_pending(&pending)?;
    view(request_checkout(pending).await?)
}

async fn request_checkout(mut pending: PendingPurchase) -> Result<PendingPurchase, String> {
    let api_base = crate::license::license_api_base()?;
    let client = crate::net::doh_http_client()?;
    let response = client
        .post(format!("{}/v1/purchases", api_base))
        .header("x-idempotency-key", &pending.idempotency_key)
        .json(&pending.input)
        .send()
        .await
        .map_err(|_| "Couldn't reach the secure checkout service.".to_string())?;
    let body: CreatePurchaseResponse = parse_json(response).await?;
    if !body.ok {
        return Err(body
            .error
            .unwrap_or_else(|| "Checkout couldn't be started.".to_string()));
    }

    let checkout_url = body
        .checkout_url
        .ok_or_else(|| "Checkout response was incomplete.".to_string())?;
    if !checkout_url_is_trusted(&checkout_url, &api_base) {
        return Err("Checkout service returned an untrusted link.".to_string());
    }
    pending.purchase_id = body.purchase_id;
    pending.recovery_token = body.recovery_token;
    pending.checkout_url = Some(checkout_url);
    pending.amount = body.amount;
    pending.currency = body.currency;
    pending.expires_at = body.expires_at;
    if pending.purchase_id.is_none()
        || pending.recovery_token.is_none()
        || pending.checkout_url.is_none()
    {
        return Err("Checkout response was incomplete.".to_string());
    }
    save_pending(&pending)?;
    Ok(pending)
}

#[tauri::command]
pub async fn resume_purchase_checkout() -> Result<PurchaseView, String> {
    let pending = load_pending()?.ok_or_else(|| "No pending checkout.".to_string())?;
    if pending.purchase_id.is_none() || pending.recovery_token.is_none() {
        return Err("Checkout has not been created yet.".to_string());
    }
    if pending
        .expires_at
        .is_some_and(|expires_at| expires_at <= unix_timestamp())
    {
        return Err("This checkout has expired. Start over to create a new one.".to_string());
    }
    view(request_checkout(pending).await?)
}

#[tauri::command]
pub fn get_pending_purchase() -> Result<Option<PurchaseView>, String> {
    load_pending()?.map(view).transpose()
}

#[tauri::command]
pub async fn poll_purchase_status() -> Result<PurchaseStatus, String> {
    request_purchase_status("status").await
}

#[tauri::command]
pub async fn reconcile_purchase_status() -> Result<PurchaseStatus, String> {
    request_purchase_status("reconcile").await
}

async fn request_purchase_status(action: &str) -> Result<PurchaseStatus, String> {
    let pending = load_pending()?.ok_or_else(|| "No pending purchase.".to_string())?;
    let purchase_id = pending
        .purchase_id
        .as_deref()
        .ok_or_else(|| "Checkout hasn't started yet.".to_string())?;
    let recovery_token = pending
        .recovery_token
        .as_deref()
        .ok_or_else(|| "Purchase recovery is unavailable.".to_string())?;

    let client = crate::net::doh_http_client()?;
    let response = client
        .post(format!(
            "{}/v1/purchases/{}/{}",
            crate::license::license_api_base()?,
            purchase_id,
            action
        ))
        .bearer_auth(recovery_token)
        .send()
        .await
        .map_err(|_| "Couldn't check payment status.".to_string())?;
    let body: StatusResponse = parse_json(response).await?;
    if !body.ok {
        return Err(body
            .error
            .unwrap_or_else(|| "Payment status is unavailable.".to_string()));
    }

    let mut activated = false;
    let mut activation_error = None;
    if let Some(key) = body.license_key.as_deref() {
        match crate::license::activate_license(key.to_string()).await {
            Ok(_) => activated = true,
            Err(error) => activation_error = Some(error),
        }
    }

    Ok(PurchaseStatus {
        state: body.state.unwrap_or_else(|| "pending".to_string()),
        provider_status: body.provider_status,
        amount: body.amount,
        currency: body.currency,
        license_key: body.license_key,
        activated,
        activation_error,
    })
}

#[tauri::command]
pub async fn resend_purchase_license() -> Result<(), String> {
    let pending = load_pending()?.ok_or_else(|| "No pending purchase.".to_string())?;
    let purchase_id = pending
        .purchase_id
        .as_deref()
        .ok_or_else(|| "Checkout hasn't started yet.".to_string())?;
    let recovery_token = pending
        .recovery_token
        .as_deref()
        .ok_or_else(|| "Purchase recovery is unavailable.".to_string())?;
    let client = crate::net::doh_http_client()?;
    let response = client
        .post(format!(
            "{}/v1/purchases/{}/resend",
            crate::license::license_api_base()?,
            purchase_id
        ))
        .bearer_auth(recovery_token)
        .send()
        .await
        .map_err(|_| "Couldn't request another delivery email.".to_string())?;
    let body: StatusResponse = parse_json(response).await?;
    if body.ok {
        Ok(())
    } else {
        Err(body
            .error
            .unwrap_or_else(|| "The delivery email couldn't be queued.".to_string()))
    }
}

#[tauri::command]
pub fn forget_pending_purchase() -> Result<(), String> {
    crate::datastore::save(STORE_SECTION, &serde_json::json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(sku: &str, seats: Option<u16>) -> PurchaseInput {
        PurchaseInput {
            sku: sku.into(),
            seats,
            email: "buyer@example.com".into(),
            phone: None,
        }
    }

    #[test]
    fn personal_offers_do_not_accept_a_client_selected_device_allowance() {
        for sku in ["pro_lifetime", "pro_membership", "investigator"] {
            assert!(validate_purchase_input(&input(sku, None)).is_ok());
            assert!(validate_purchase_input(&input(sku, Some(3))).is_err());
        }
    }

    #[test]
    fn fleet_is_the_only_seat_based_offer() {
        assert!(validate_purchase_input(&input("fleet", Some(1))).is_ok());
        assert!(validate_purchase_input(&input("fleet", Some(50))).is_ok());
        assert!(validate_purchase_input(&input("fleet", Some(51))).is_err());
        assert!(validate_purchase_input(&input("fleet", None)).is_err());
    }

    #[test]
    fn checkout_url_must_be_https_and_match_the_pinned_origin() {
        let api_base = "https://licensing.servalabs.example";
        assert!(checkout_url_is_trusted(
            "https://licensing.servalabs.example/v1/checkout/abc",
            api_base
        ));
        assert!(!checkout_url_is_trusted(
            "http://licensing.servalabs.example/v1/checkout/abc",
            api_base
        ));
        assert!(!checkout_url_is_trusted(
            "https://checkout.attacker.example/v1/checkout/abc",
            api_base
        ));
    }

    #[test]
    fn catalog_offers_must_have_complete_server_display_data() {
        let offer = CatalogOffer {
            sku: "fleet".into(),
            name: "Fleet".into(),
            price_label: "Server-calculated".into(),
            detail: "Managed Windows endpoints and Netwall.".into(),
            device_rule: "One device per seat.".into(),
            checkout_eligible: true,
            min_seats: Some(1),
            max_seats: Some(50),
            seat_pricing_label: Some("Server-calculated by device count.".into()),
        };
        assert!(validate_catalog_offer(&offer));
        let invalid = CatalogOffer {
            price_label: String::new(),
            ..offer
        };
        assert!(!validate_catalog_offer(&invalid));
    }

    #[test]
    fn purchase_view_never_exposes_recovery_token() {
        let pending = PendingPurchase {
            idempotency_key: "idem-secret".into(),
            input: PurchaseInput {
                sku: "pro_lifetime".into(),
                seats: None,
                email: "buyer@example.com".into(),
                phone: None,
            },
            purchase_id: Some("purchase-id".into()),
            recovery_token: Some("recovery-secret".into()),
            checkout_url: Some("https://checkout.example/start".into()),
            amount: Some(450_000),
            currency: Some("INR".into()),
            expires_at: Some(1),
        };
        let exposed = serde_json::to_string(&PurchaseView {
            purchase_id: pending.purchase_id,
            sku: pending.input.sku,
            seats: pending.input.seats,
            checkout_url: pending.checkout_url,
            amount: pending.amount,
            currency: pending.currency,
            expires_at: pending.expires_at,
        })
        .expect("serialize");
        assert!(!exposed.contains("recovery-secret"));
        assert!(!exposed.contains("idem-secret"));
    }
}
