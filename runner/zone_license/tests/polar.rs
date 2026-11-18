use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Duration, TimeZone, Utc};
use serde_json::{Value, json};
use zone_license::decision::{Decision, decide};
use zone_license::grant::Grant;
use zone_license::ignored::Ignored;
use zone_license::order_error::OrderError;
use zone_license::polar::{
    Event, Headers, ID_HEADER, SIGNATURE_HEADER, SignatureError, TIMESTAMP_HEADER, WebhookSecret,
    signature, verify,
};

const CREATE: &[u8] = include_bytes!("fixtures/polar/order_paid_subscription_create.json");
const CYCLE: &[u8] = include_bytes!("fixtures/polar/order_paid_subscription_cycle.json");
const UPDATE: &[u8] = include_bytes!("fixtures/polar/order_paid_subscription_update.json");
const PURCHASE: &[u8] = include_bytes!("fixtures/polar/order_paid_purchase.json");
const UNPAID: &[u8] = include_bytes!("fixtures/polar/order_paid_unpaid.json");
const NO_SEATS: &[u8] = include_bytes!("fixtures/polar/order_paid_no_seats.json");
const NO_EMAIL: &[u8] = include_bytes!("fixtures/polar/order_paid_no_email.json");
const SUBSCRIPTION_UPDATED: &[u8] = include_bytes!("fixtures/polar/subscription_updated.json");
const SUBSCRIPTION_CANCELED: &[u8] = include_bytes!("fixtures/polar/subscription_canceled.json");
const SUBSCRIPTION_CYCLED: &[u8] = include_bytes!("fixtures/polar/subscription_cycled.json");
const ORDER_CREATED: &[u8] = include_bytes!("fixtures/polar/order_created.json");

const ID: &str = "msg_2kWm9Q4tYpS3vB7xN1cR8dLe";
const TIMESTAMP: i64 = 1_795_003_199;
const LEGACY_SECRET: &str = "whsec_9a1b2c3d4e5f60718293a4b5c6d7e8f9";
const STANDARD_SECRET: &str = "whsec_AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
const STANDARD_KEY: [u8; 32] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
];
const LEGACY_SIGNATURE: &str = "v1,7VUsba2aQ5nn5Tkrb5lg9ldnoRKZvCrb5Fpzbt3pMBA=";
const STANDARD_SIGNATURE: &str = "v1,ju5wgTiv0nbx+Q4czFcw8fzQlv0TH/lveJDGR7ACX9c=";
const OTHER_SIGNATURE: &str = "v1,dRIigTnuNJR6SEyRZ4g29+Edol/EBaHEq+hjQR9VN7M=";

const ORDER: &str = "3d6e9f21-8a4b-4c1d-9e2f-0a1b2c3d4e01";
const CYCLE_ORDER: &str = "3d6e9f21-8a4b-4c1d-9e2f-0a1b2c3d4e02";
const UPDATE_ORDER: &str = "3d6e9f21-8a4b-4c1d-9e2f-0a1b2c3d4e03";
const SUBSCRIPTION: &str = "b7e4c2a1-3d5f-4e6a-9b8c-0d1e2f3a4b5c";
const PRODUCT: &str = "5f0c8a9e-6b1d-4c2e-9a3f-7d4e1b2c3a40";
const EMAIL: &str = "billing@acme.test";
const NO_EMAIL_ORDER: &str = "3d6e9f21-8a4b-4c1d-9e2f-0a1b2c3d4e07";
const NO_SEATS_ORDER: &str = "3d6e9f21-8a4b-4c1d-9e2f-0a1b2c3d4e06";

fn now() -> DateTime<Utc> {
    Utc.timestamp_opt(TIMESTAMP, 0).unwrap()
}

fn headers(signature: &str) -> Headers {
    Headers {
        id: Some(ID.to_owned()),
        timestamp: Some(TIMESTAMP.to_string()),
        signature: Some(signature.to_owned()),
    }
}

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text).unwrap().to_utc()
}

fn event(body: &[u8]) -> Event {
    Event::parse(body).unwrap()
}

fn edited(body: &[u8], edit: impl FnOnce(&mut Value)) -> Event {
    let mut value: Value = serde_json::from_slice(body).unwrap();
    edit(&mut value);
    serde_json::from_value(value).unwrap()
}

fn grant(order: &str, seats: u32, start: &str, end: &str) -> Grant {
    Grant {
        order: order.to_owned(),
        subscription: SUBSCRIPTION.to_owned(),
        customer: "Acme Ltd".to_owned(),
        email: EMAIL.to_owned(),
        seats,
        period_start: at(start),
        period_end: at(end),
    }
}

fn first_year() -> Grant {
    grant(ORDER, 5, "2026-11-18T11:59:58Z", "2027-11-18T11:59:58Z")
}

fn products() -> [String; 1] {
    [PRODUCT.to_owned()]
}

fn issued(event: &Event) -> Grant {
    match decide(event, &products()) {
        Ok(Decision::Issue(grant)) => grant,
        other => panic!("expected a grant, got {other:?}"),
    }
}

#[test]
fn accepts_a_legacy_secret() {
    let secret = WebhookSecret::new(LEGACY_SECRET);
    assert_eq!(
        verify(&secret, &headers(LEGACY_SIGNATURE), CREATE, now()),
        Ok(())
    );
}

#[test]
fn accepts_a_standard_secret() {
    let secret = WebhookSecret::new(STANDARD_SECRET);
    assert_eq!(
        verify(&secret, &headers(STANDARD_SIGNATURE), CREATE, now()),
        Ok(())
    );
}

#[test]
fn signs_like_openssl() {
    assert_eq!(
        signature(LEGACY_SECRET.as_bytes(), ID, TIMESTAMP, CREATE),
        LEGACY_SIGNATURE
    );
    assert_eq!(
        signature(&STANDARD_KEY, ID, TIMESTAMP, CREATE),
        STANDARD_SIGNATURE
    );
    assert_eq!(
        signature(LEGACY_SECRET.as_bytes(), "msg_1", TIMESTAMP, br#"{"a":1}"#),
        "v1,dWZx+ucPxdim1hm4/psHaVApO0h5SP5bkVxoX1siz+8="
    );
}

#[test]
fn standard_secret_decodes_to_the_raw_key() {
    let remainder = STANDARD_SECRET.strip_prefix("whsec_").unwrap();
    assert_eq!(STANDARD.decode(remainder).unwrap(), STANDARD_KEY);
}

#[test]
fn accepts_any_matching_entry_among_several() {
    let secret = WebhookSecret::new(LEGACY_SECRET);
    let header = format!("{OTHER_SIGNATURE} {LEGACY_SIGNATURE}");
    assert_eq!(verify(&secret, &headers(&header), CREATE, now()), Ok(()));
    let header = format!("{LEGACY_SIGNATURE} {OTHER_SIGNATURE}");
    assert_eq!(verify(&secret, &headers(&header), CREATE, now()), Ok(()));
}

#[test]
fn refuses_a_tampered_body() {
    let secret = WebhookSecret::new(LEGACY_SECRET);
    let mut body = CREATE.to_vec();
    let seats = body
        .windows(10)
        .position(|window| window == b"\"seats\": 5")
        .unwrap();
    body[seats + 9] = b'9';
    assert_eq!(
        verify(&secret, &headers(LEGACY_SIGNATURE), &body, now()),
        Err(SignatureError::Mismatch)
    );
}

#[test]
fn refuses_a_tampered_id() {
    let secret = WebhookSecret::new(LEGACY_SECRET);
    let headers = Headers {
        id: Some("msg_replayed".to_owned()),
        ..headers(LEGACY_SIGNATURE)
    };
    assert_eq!(
        verify(&secret, &headers, CREATE, now()),
        Err(SignatureError::Mismatch)
    );
}

#[test]
fn refuses_the_wrong_secret() {
    let secret = WebhookSecret::new("whsec_someone-else-entirely");
    assert_eq!(
        verify(&secret, &headers(LEGACY_SIGNATURE), CREATE, now()),
        Err(SignatureError::Mismatch)
    );
    let secret = WebhookSecret::new(STANDARD_SECRET);
    assert_eq!(
        verify(&secret, &headers(LEGACY_SIGNATURE), CREATE, now()),
        Err(SignatureError::Mismatch)
    );
}

#[test]
fn refuses_a_timestamp_301_seconds_old() {
    let secret = WebhookSecret::new(LEGACY_SECRET);
    assert_eq!(
        verify(
            &secret,
            &headers(LEGACY_SIGNATURE),
            CREATE,
            now() + Duration::seconds(301)
        ),
        Err(SignatureError::Stale)
    );
}

#[test]
fn refuses_a_timestamp_301_seconds_ahead() {
    let secret = WebhookSecret::new(LEGACY_SECRET);
    assert_eq!(
        verify(
            &secret,
            &headers(LEGACY_SIGNATURE),
            CREATE,
            now() - Duration::seconds(301)
        ),
        Err(SignatureError::Stale)
    );
}

#[test]
fn accepts_the_300_second_boundary() {
    let secret = WebhookSecret::new(LEGACY_SECRET);
    for offset in [-300, 300] {
        assert_eq!(
            verify(
                &secret,
                &headers(LEGACY_SIGNATURE),
                CREATE,
                now() + Duration::seconds(offset)
            ),
            Ok(()),
            "offset {offset}"
        );
    }
}

#[test]
fn refuses_a_non_numeric_timestamp() {
    let secret = WebhookSecret::new(LEGACY_SECRET);
    for timestamp in ["soon", "1795003199.0", "1.795e9"] {
        let headers = Headers {
            timestamp: Some(timestamp.to_owned()),
            ..headers(LEGACY_SIGNATURE)
        };
        assert_eq!(
            verify(&secret, &headers, CREATE, now()),
            Err(SignatureError::Timestamp),
            "timestamp {timestamp}"
        );
    }
}

#[test]
fn refuses_each_missing_header() {
    let secret = WebhookSecret::new(LEGACY_SECRET);
    let cases = [
        (
            Headers {
                id: None,
                ..headers(LEGACY_SIGNATURE)
            },
            ID_HEADER,
        ),
        (
            Headers {
                timestamp: None,
                ..headers(LEGACY_SIGNATURE)
            },
            TIMESTAMP_HEADER,
        ),
        (
            Headers {
                signature: None,
                ..headers(LEGACY_SIGNATURE)
            },
            SIGNATURE_HEADER,
        ),
        (
            Headers {
                signature: Some(String::new()),
                ..headers(LEGACY_SIGNATURE)
            },
            SIGNATURE_HEADER,
        ),
    ];
    for (headers, name) in cases {
        assert_eq!(
            verify(&secret, &headers, CREATE, now()),
            Err(SignatureError::Missing(name)),
            "header {name}"
        );
    }
}

#[test]
fn reads_headers_through_a_lookup() {
    let headers = Headers::read(|name| match name {
        ID_HEADER => Some(ID),
        TIMESTAMP_HEADER => Some("1795003199"),
        SIGNATURE_HEADER => Some(LEGACY_SIGNATURE),
        _ => None,
    });
    let secret = WebhookSecret::new(LEGACY_SECRET);
    assert_eq!(verify(&secret, &headers, CREATE, now()), Ok(()));
}

#[test]
fn secret_debug_hides_the_keys() {
    let rendered = format!("{:?}", WebhookSecret::new(LEGACY_SECRET));
    assert!(!rendered.contains("9a1b2c3d"), "{rendered}");
}

#[test]
fn subscription_create_issues_the_first_year() {
    assert_eq!(
        decide(&event(CREATE), &products()),
        Ok(Decision::Issue(first_year()))
    );
}

#[test]
fn subscription_cycle_issues_the_next_period() {
    assert_eq!(
        decide(&event(CYCLE), &products()),
        Ok(Decision::Issue(grant(
            CYCLE_ORDER,
            5,
            "2027-11-18T11:59:58Z",
            "2028-11-18T11:59:58Z"
        )))
    );
}

#[test]
fn subscription_update_issues_the_new_seat_count() {
    assert_eq!(
        decide(&event(UPDATE), &products()),
        Ok(Decision::Issue(grant(
            UPDATE_ORDER,
            8,
            "2026-11-18T11:59:58Z",
            "2027-11-18T11:59:58Z"
        )))
    );
}

#[test]
fn one_time_purchase_is_ignored() {
    assert_eq!(
        decide(&event(PURCHASE), &products()),
        Ok(Decision::Ignore(Ignored::BillingReason))
    );
}

#[test]
fn unpaid_order_is_ignored() {
    assert_eq!(
        decide(&event(UNPAID), &products()),
        Ok(Decision::Ignore(Ignored::Unpaid))
    );
}

fn unaddressed() -> OrderError {
    OrderError::Email {
        order: ORDER.to_owned(),
    }
}

fn misconfigured(order: &str) -> Result<Decision, OrderError> {
    Err(OrderError::Misconfigured {
        order: order.to_owned(),
        product: PRODUCT.to_owned(),
    })
}

#[test]
fn a_listed_product_without_seats_is_misconfigured() {
    assert_eq!(
        decide(&event(NO_SEATS), &products()),
        misconfigured(NO_SEATS_ORDER)
    );
}

#[test]
fn an_unlisted_product_without_seats_is_ignored() {
    assert_eq!(
        decide(&event(NO_SEATS), &["another-product".to_owned()]),
        Ok(Decision::Ignore(Ignored::Product))
    );
}

#[test]
fn order_without_email_is_an_error() {
    assert_eq!(
        decide(&event(NO_EMAIL), &products()),
        Err(OrderError::Email {
            order: NO_EMAIL_ORDER.to_owned()
        })
    );
}

#[test]
fn other_event_types_are_ignored() {
    for body in [
        SUBSCRIPTION_UPDATED,
        SUBSCRIPTION_CANCELED,
        SUBSCRIPTION_CYCLED,
        ORDER_CREATED,
    ] {
        assert_eq!(
            decide(&event(body), &products()),
            Ok(Decision::Ignore(Ignored::EventType))
        );
    }
}

#[test]
fn meter_cycle_and_unknown_reasons_are_ignored() {
    for reason in ["subscription_meter_cycle", "subscription_pause"] {
        let event = edited(CREATE, |value| {
            value["data"]["billing_reason"] = json!(reason)
        });
        assert_eq!(
            decide(&event, &products()),
            Ok(Decision::Ignore(Ignored::BillingReason)),
            "reason {reason}"
        );
    }
}

#[test]
fn customer_falls_back_to_the_customer_billing_name() {
    let event = edited(CREATE, |value| {
        value["data"]["billing_name"] = Value::Null;
        value["data"]["customer"]["billing_name"] = json!("Acme Holdings");
    });
    assert_eq!(issued(&event).customer, "Acme Holdings");
}

#[test]
fn customer_falls_back_to_the_customer_name() {
    let event = edited(CREATE, |value| {
        value["data"]["billing_name"] = json!("   ");
        value["data"]["customer"]["billing_name"] = Value::Null;
    });
    assert_eq!(issued(&event).customer, "Jane Doe");
}

#[test]
fn customer_falls_back_to_the_email() {
    let event = edited(CREATE, |value| {
        value["data"]["billing_name"] = Value::Null;
        value["data"]["customer"]["billing_name"] = json!("");
        value["data"]["customer"]["name"] = Value::Null;
    });
    assert_eq!(issued(&event).customer, EMAIL);
}

#[test]
fn listed_product_issues() {
    assert_eq!(
        decide(&event(CREATE), &[PRODUCT.to_owned()]),
        Ok(Decision::Issue(first_year()))
    );
}

#[test]
fn unlisted_product_is_ignored() {
    assert_eq!(
        decide(&event(CREATE), &["another-product".to_owned()]),
        Ok(Decision::Ignore(Ignored::Product))
    );
}

#[test]
fn order_without_a_product_is_ignored_when_products_are_listed() {
    let event = edited(CREATE, |value| value["data"]["product_id"] = Value::Null);
    assert_eq!(
        decide(&event, &products()),
        Ok(Decision::Ignore(Ignored::Product))
    );
}

#[test]
fn an_empty_product_list_issues_nothing() {
    assert_eq!(
        decide(&event(CREATE), &[]),
        Ok(Decision::Ignore(Ignored::Product))
    );
}

#[test]
fn order_seats_without_subscription_seats_are_misconfigured() {
    let event = edited(CREATE, |value| {
        value["data"]["subscription"]["seats"] = Value::Null;
        value["data"]["seats"] = json!(3);
    });
    assert_eq!(decide(&event, &products()), misconfigured(ORDER));
}

#[test]
fn a_proration_delta_on_the_order_does_not_change_the_seats() {
    let event = edited(UPDATE, |value| {
        value["data"]["seats"] = json!(3);
    });
    assert_eq!(issued(&event).seats, 8);
}

#[test]
fn zero_seats_are_misconfigured() {
    let event = edited(CREATE, |value| {
        value["data"]["subscription"]["seats"] = json!(0);
    });
    assert_eq!(decide(&event, &products()), misconfigured(ORDER));
}

#[test]
fn a_listed_product_without_a_subscription_is_misconfigured() {
    let event = edited(CREATE, |value| {
        value["data"]["subscription"] = Value::Null;
    });
    assert_eq!(decide(&event, &products()), misconfigured(ORDER));
}

#[test]
fn blank_or_absent_email_is_an_error() {
    let blank = edited(CREATE, |value| {
        value["data"]["customer"]["email"] = json!("  ")
    });
    assert_eq!(decide(&blank, &products()), Err(unaddressed()));
    let absent = edited(CREATE, |value| {
        value["data"]["customer"]
            .as_object_mut()
            .unwrap()
            .remove("email");
    });
    assert_eq!(decide(&absent, &products()), Err(unaddressed()));
}

#[test]
fn a_trialing_subscription_is_ignored() {
    let event = edited(CREATE, |value| {
        value["data"]["subscription"]["status"] = json!("trialing");
        value["data"]["total_amount"] = json!(0);
    });
    assert_eq!(
        decide(&event, &products()),
        Ok(Decision::Ignore(Ignored::Trial))
    );
}

#[test]
fn the_final_order_of_a_canceled_subscription_is_ignored() {
    let event = edited(CYCLE, |value| {
        value["data"]["subscription"]["status"] = json!("canceled");
        value["data"]["subscription"]["cancel_at_period_end"] = json!(true);
    });
    assert_eq!(
        decide(&event, &products()),
        Ok(Decision::Ignore(Ignored::Canceled))
    );
}

#[test]
fn a_subscription_due_to_cancel_still_issues_while_active() {
    let event = edited(UPDATE, |value| {
        value["data"]["subscription"]["cancel_at_period_end"] = json!(true);
    });
    assert_eq!(issued(&event).seats, 8);
}

#[test]
fn subscription_status_does_not_gate_issuing() {
    let event = edited(CREATE, |value| {
        value["data"]["subscription"]["status"] = json!("past_due");
    });
    assert_eq!(issued(&event), first_year());
}

#[test]
fn fields_the_signer_never_reads_may_be_absent() {
    let mut value: Value = serde_json::from_slice(CREATE).unwrap();
    for field in ["timestamp", "api_version"] {
        value.as_object_mut().unwrap().remove(field);
    }
    let data = value["data"].as_object_mut().unwrap();
    for field in ["status", "subscription_id", "created_at", "seats"] {
        data.remove(field);
    }
    data["customer"].as_object_mut().unwrap().remove("id");
    data["subscription"]
        .as_object_mut()
        .unwrap()
        .remove("cancel_at_period_end");
    let event = Event::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(issued(&event), first_year());
}

#[test]
fn malformed_order_is_an_error() {
    let event = edited(CREATE, |value| value["data"]["paid"] = json!("yes"));
    assert_eq!(decide(&event, &products()), Err(OrderError::Malformed));
    let event = edited(CREATE, |value| {
        value["data"]["subscription"]["current_period_end"] = json!("next year")
    });
    assert_eq!(decide(&event, &products()), Err(OrderError::Malformed));
}

#[test]
fn verified_body_maps_to_a_grant() {
    let secret = WebhookSecret::new(STANDARD_SECRET);
    verify(&secret, &headers(STANDARD_SIGNATURE), CREATE, now()).unwrap();
    assert_eq!(
        decide(&Event::parse(CREATE).unwrap(), &[PRODUCT.to_owned()]),
        Ok(Decision::Issue(first_year()))
    );
}
