//! Edge cases for HASH-002 to HASH-009 and HASH-014, driven through HTTP only.
//! The happy paths live in `integration.rs`, the shared vectors in `vectors.rs` and
//! the HASH-014 log checks in `logging.rs`.

mod common;

use std::sync::Arc;

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::Method;
use serde_json::{Value, json};

use common::*;

// HASH-002: the three client-caused errors on POST /hash use the documented shape.
#[tokio::test]
async fn post_errors_have_the_documented_shape() {
    let s = spawn_server().await;

    let no_auth = s.client.post(s.url("/hash")).body(post_body(1, 1, [0; 32]));
    assert_error_shape(no_auth.send().await.unwrap(), 401, "unauthorized").await;

    let token = s.token(DEVICE_A, "device");
    assert_error_shape(s.post_raw(&token, vec![0; 3]).await, 400, "invalid_body").await;

    assert_eq!(s.post(DEVICE_A, 1, 1, [1; 32]).await.status(), 201);
    assert_error_shape(
        s.post(DEVICE_A, 2, 1, [1; 32]).await,
        409,
        "sequence_conflict",
    )
    .await;
}

// HASH-002: query errors and auth errors on GET and DELETE use the documented shape.
#[tokio::test]
async fn query_and_auth_errors_have_the_documented_shape() {
    let s = spawn_server().await;
    let device_token = s.token(DEVICE_A, "device");

    assert_error_shape(
        s.get_devices("devices=not-a-uuid").await,
        400,
        "invalid_query",
    )
    .await;
    assert_error_shape(s.get_devices("").await, 400, "invalid_query").await;
    assert_error_shape(s.delete_device("device=nope").await, 400, "invalid_query").await;
    assert_error_shape(s.delete_device("").await, 400, "invalid_query").await;

    for method in [Method::GET, Method::DELETE] {
        let resp = s
            .client
            .request(
                method,
                s.url(&format!("/hash?devices={DEVICE_A}&device={DEVICE_A}")),
            )
            .header("Authorization", format!("Bearer {device_token}"))
            .send()
            .await
            .unwrap();
        assert_error_shape(resp, 401, "unauthorized").await;
    }
}

// HASH-002, HASH-004: the 410 for an unsupported version is an error response too, and
// keeps the `error` field that old clients show.
#[tokio::test]
async fn gone_response_has_the_documented_error_shape() {
    let s = spawn_server().await;
    let resp = s.client.get(s.url("/v2/hash")).send().await.unwrap();
    assert_eq!(resp.status(), 410);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "gone", "{body}");
    assert!(body["message"].as_str().is_some_and(|m| !m.is_empty()));
    assert_eq!(body["error"], body["message"], "{body}");
}

// HASH-002: unknown routes and unsupported methods are error responses too.
#[tokio::test]
async fn unknown_route_and_method_have_the_documented_error_shape() {
    let s = spawn_server().await;
    for (method, path, status, code) in [
        (Method::GET, "/nope", 404, "not_found"),
        (Method::GET, "/v0.1/nope", 404, "not_found"),
        (Method::PUT, "/hash", 405, "method_not_allowed"),
        (Method::POST, "/", 405, "method_not_allowed"),
    ] {
        let resp = s
            .client
            .request(method.clone(), s.url(path))
            .send()
            .await
            .unwrap();
        assert_error_shape(resp, status, code).await;
    }
}

// HASH-002: a 405 still names the methods the route allows.
#[tokio::test]
async fn method_not_allowed_keeps_the_allow_header() {
    let s = spawn_server().await;
    let resp = s.client.put(s.url("/hash")).send().await.unwrap();
    assert_eq!(resp.status(), 405);
    let allow = resp.headers()["allow"].to_str().unwrap().to_string();
    for method in ["GET", "POST", "DELETE"] {
        assert!(allow.contains(method), "{allow}");
    }
}

// HASH-002: a repeated query parameter is a malformed query and gets the error shape.
#[tokio::test]
async fn repeated_query_parameter_has_the_documented_error_shape() {
    let s = spawn_server().await;
    let get = s
        .get_devices(&format!("devices={DEVICE_A}&devices={DEVICE_B}"))
        .await;
    assert_error_shape(get, 400, "invalid_query").await;

    let delete = s
        .client
        .delete(s.url(&format!("/hash?device={DEVICE_A}&device={DEVICE_B}")))
        .header(
            "Authorization",
            format!("Bearer {}", s.token("api", "server")),
        )
        .send()
        .await
        .unwrap();
    assert_error_shape(delete, 400, "invalid_query").await;
}

// HASH-002: a body too large to read is an invalid body, and the token is checked first.
#[tokio::test]
async fn oversized_body_has_the_documented_error_shape() {
    let s = spawn_server().await;
    let token = s.token(DEVICE_A, "device");
    let oversized = vec![0; 2 * 1024 * 1024 + 1];
    let resp = s.post_raw(&token, oversized.clone()).await;
    assert_error_shape(resp, 400, "invalid_body").await;

    let resp = s
        .client
        .post(s.url("/hash"))
        .body(oversized)
        .send()
        .await
        .unwrap();
    assert_error_shape(resp, 401, "unauthorized").await;
}

// HASH-002: authentication comes before a query the server cannot parse.
#[tokio::test]
async fn repeated_query_parameter_without_a_token_is_unauthorized() {
    let s = spawn_server().await;
    let resp = s
        .client
        .get(s.url(&format!("/hash?devices={DEVICE_A}&devices={DEVICE_B}")))
        .send()
        .await
        .unwrap();
    assert_error_shape(resp, 401, "unauthorized").await;
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// Tokens of type `typ` that must all be rejected with 401: wrong key, expired, wrong
/// algorithm, tampered, and malformed claims.
fn jwt_cases(server_key_pem: &str, typ: &str) -> Vec<(&'static str, String)> {
    let exp = now() + 3600;
    let long_ago = now() - 7200;
    let (other_private, _) = new_signing_key_pem();
    let (_, public_pem) = new_signing_key_pem();
    let good = json!({ "sub": DEVICE_A, "type": typ, "exp": exp });

    let hs256 = jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        &good,
        &EncodingKey::from_secret(public_pem.as_bytes()),
    )
    .unwrap();

    // alg=none with no signature, built by hand.
    let none = format!(
        "{}.{}.",
        base64url(br#"{"alg":"none","typ":"JWT"}"#),
        base64url(good.to_string().as_bytes())
    );

    let mut tampered = sign_eddsa(&good, server_key_pem);
    // Flip the first character of the signature.
    let dot = tampered.rfind('.').unwrap() + 1;
    let first = tampered.as_bytes()[dot];
    tampered.replace_range(dot..=dot, if first == b'A' { "B" } else { "A" });

    let mut cases = vec![
        (
            "signed by a different key",
            sign_eddsa(&good, &other_private),
        ),
        (
            "expired",
            sign_eddsa(
                &json!({ "sub": DEVICE_A, "type": typ, "exp": long_ago }),
                server_key_pem,
            ),
        ),
        ("HS256 instead of EdDSA", hs256),
        ("alg none", none),
        ("tampered signature", tampered),
        (
            "missing type claim",
            sign_eddsa(&json!({ "sub": DEVICE_A, "exp": exp }), server_key_pem),
        ),
        (
            "unknown type claim",
            sign_eddsa(
                &json!({ "sub": DEVICE_A, "type": "admin", "exp": exp }),
                server_key_pem,
            ),
        ),
        (
            "type claim in the wrong case",
            sign_eddsa(
                &json!({ "sub": DEVICE_A, "type": typ.to_uppercase(), "exp": exp }),
                server_key_pem,
            ),
        ),
        (
            "type claim of the wrong JSON type",
            sign_eddsa(
                &json!({ "sub": DEVICE_A, "type": [typ], "exp": exp }),
                server_key_pem,
            ),
        ),
        ("garbage", "not.a.jwt".to_string()),
    ];
    // The sub of a `server` token is ignored, so only a device token needs one.
    if typ == "device" {
        cases.push((
            "missing sub claim",
            sign_eddsa(&json!({ "type": typ, "exp": exp }), server_key_pem),
        ));
    }
    cases
}

// HASH-003, HASH-006: POST /hash rejects every kind of invalid JWT with 401 and
// changes no state.
#[tokio::test]
async fn post_rejects_invalid_jwts() {
    let s = spawn_server().await;
    for (name, token) in jwt_cases(&s.signing_key_pem, "device") {
        let resp = s.post_raw(&token, post_body(10, 1, [7; 32])).await;
        assert_eq!(resp.status(), 401, "{name}");
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["code"], "unauthorized", "{name}");
    }
    let state = s.state(DEVICE_A).await;
    assert_eq!(state["seq"], 0, "a rejected request must not write");
}

// HASH-003, HASH-007: GET /hash rejects every kind of invalid JWT with 401.
#[tokio::test]
async fn get_rejects_invalid_jwts() {
    let s = spawn_server().await;
    for (name, token) in jwt_cases(&s.signing_key_pem, "server") {
        let resp = s
            .client
            .get(s.url(&format!("/hash?devices={DEVICE_A}")))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401, "{name}");
    }
}

// HASH-003, HASH-008: DELETE /hash rejects every kind of invalid JWT with 401 and
// resets nothing.
#[tokio::test]
async fn delete_rejects_invalid_jwts() {
    let s = spawn_server().await;
    assert_eq!(s.post(DEVICE_A, 10, 3, [4; 32]).await.status(), 201);
    for (name, token) in jwt_cases(&s.signing_key_pem, "server") {
        let resp = s
            .client
            .delete(s.url(&format!("/hash?device={DEVICE_A}")))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401, "{name}");
    }
    assert_eq!(s.state(DEVICE_A).await["seq"], 3);
}

// HASH-006, HASH-007, HASH-008: the Authorization header must be `Bearer <token>`.
#[tokio::test]
async fn authorization_header_must_be_a_bearer_token() {
    let s = spawn_server().await;
    let device = s.token(DEVICE_A, "device");
    let server = s.token("x", "server");
    let cases = [
        ("Basic scheme", format!("Basic {device}")),
        ("no scheme", device.clone()),
        ("empty bearer", "Bearer ".to_string()),
        ("empty value", String::new()),
    ];
    for (name, value) in cases {
        let resp = s
            .client
            .post(s.url("/hash"))
            .header("Authorization", &value)
            .body(post_body(1, 1, [1; 32]))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401, "POST {name}");

        let resp = s
            .client
            .get(s.url(&format!("/hash?devices={DEVICE_A}")))
            .header("Authorization", value.replace(&device, &server))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401, "GET {name}");
    }
}

// HASH-006, HASH-007, HASH-008: a request with no Authorization header is a 401 on
// every method, checked before the query is validated.
#[tokio::test]
async fn missing_authorization_header_is_401_on_every_method() {
    let s = spawn_server().await;
    for (method, path) in [
        (Method::POST, "/hash"),
        (Method::GET, "/hash?devices=not-a-uuid"),
        (Method::GET, "/hash"),
        (Method::DELETE, "/hash?device=not-a-uuid"),
        (Method::DELETE, "/hash"),
    ] {
        let resp = s
            .client
            .request(method.clone(), s.url(path))
            .body(post_body(1, 1, [1; 32]))
            .send()
            .await
            .unwrap();
        assert_error_shape(resp, 401, "unauthorized").await;
    }
}

// HASH-007: GET /hash needs a `server` token; a device token is not enough.
#[tokio::test]
async fn get_rejects_a_device_typed_token() {
    let s = spawn_server().await;
    let token = s.token(DEVICE_A, "device");
    let resp = s
        .client
        .get(s.url(&format!("/hash?devices={DEVICE_A}")))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_error_shape(resp, 401, "unauthorized").await;
}

// HASH-007, HASH-008: the sub claim of a server token is ignored, whatever it holds.
#[tokio::test]
async fn server_token_sub_is_ignored() {
    let s = spawn_server().await;
    assert_eq!(s.post(DEVICE_A, 10, 2, [4; 32]).await.status(), 201);
    for sub in ["", "ignored", DEVICE_B, "not a uuid", DEVICE_A] {
        let token = s.token(sub, "server");
        let get = s
            .client
            .get(s.url(&format!("/hash?devices={DEVICE_A}")))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(get.status(), 200, "GET with sub {sub:?}");
        let body: Value = get.json().await.unwrap();
        assert_eq!(body[DEVICE_A]["seq"], 2);
    }
    // DELETE of another device is not blocked by a sub naming a third device.
    let token = s.token(DEVICE_B, "server");
    let resp = s
        .client
        .delete(s.url(&format!("/hash?device={DEVICE_A}")))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

// HASH-007, HASH-008: a server token does not need a sub at all, or a string one.
#[tokio::test]
async fn server_token_without_a_string_sub_is_accepted() {
    let s = spawn_server().await;
    for claims in [
        json!({ "type": "server", "exp": now() + 3600 }),
        json!({ "sub": 42, "type": "server", "exp": now() + 3600 }),
    ] {
        let token = s.token_with(claims.clone());
        let resp = s
            .client
            .get(s.url(&format!("/hash?devices={DEVICE_A}")))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "{claims}");
    }
}

// HASH-006: a device token must name its device with a string sub.
#[tokio::test]
async fn device_token_with_a_non_string_sub_is_rejected() {
    let s = spawn_server().await;
    let token = s.token_with(json!({ "sub": 42, "type": "device", "exp": now() + 3600 }));
    let resp = s.post_raw(&token, post_body(1, 1, [0; 32])).await;
    assert_error_shape(resp, 401, "unauthorized").await;
}

// HASH-006: the token's sub is the device id, so two devices keep separate state.
#[tokio::test]
async fn devices_are_isolated_by_token_sub() {
    let s = spawn_server().await;
    assert_eq!(s.post(DEVICE_A, 100, 5, [1; 32]).await.status(), 201);
    // Device B starts from scratch: seq 1 is fine even though A is at 5.
    assert_eq!(s.post(DEVICE_B, 200, 1, [2; 32]).await.status(), 201);

    let a = s.state(DEVICE_A).await;
    let b = s.state(DEVICE_B).await;
    assert_eq!(a["seq"], 5);
    assert_eq!(a["hash"], chain([0; 32], [1; 32]));
    assert_eq!(b["seq"], 1);
    assert_eq!(b["hash"], chain([0; 32], [2; 32]));
    assert_eq!(b["last_received"], 200);
}

// HASH-006: the body is exactly 40 bytes; anything else is a 400 invalid_body that
// leaves the stored state unchanged.
#[tokio::test]
async fn post_rejects_every_body_length_but_40() {
    let s = spawn_server().await;
    let token = s.token(DEVICE_A, "device");
    for len in [0usize, 1, 8, 39, 41, 48, 80, 4096] {
        let resp = s.post_raw(&token, vec![1u8; len]).await;
        assert_error_shape(resp, 400, "invalid_body").await;
    }
    let state = s.state(DEVICE_A).await;
    assert_eq!(state["seq"], 0);
    assert_eq!(state["hash"], ZERO_HEX);
    // A valid body still works afterwards.
    assert_eq!(s.post(DEVICE_A, 1, 1, [1; 32]).await.status(), 201);
}

// HASH-006: the integer fields are little-endian: bytes [0,0,0,1] are 0x01000000.
#[tokio::test]
async fn post_integers_are_little_endian() {
    let s = spawn_server().await;
    let token = s.token(DEVICE_A, "device");
    let mut body = Vec::new();
    body.extend_from_slice(&[0x78, 0x56, 0x34, 0x12]); // unix_time 0x12345678
    body.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]); // seq 256
    body.extend_from_slice(&[9u8; 32]);
    assert_eq!(s.post_raw(&token, body).await.status(), 201);
    let state = s.state(DEVICE_A).await;
    assert_eq!(state["seq"], 256);
    assert_eq!(state["last_received"], 0x1234_5678u32);
}

// HASH-006: seq must be strictly greater than the last accepted one. Each row is a
// run of (seq, expected status) against one fresh device.
#[tokio::test]
async fn post_seq_must_strictly_increase() {
    let s = spawn_server().await;
    type SeqRun<'a> = (&'a str, &'a [(u32, u16)], u32);
    let runs: &[SeqRun] = &[
        (
            "zero is never valid first",
            &[(0, 409), (0, 409), (1, 201)],
            1,
        ),
        ("equal is rejected", &[(1, 201), (1, 409)], 1),
        (
            "lower is rejected",
            &[(5, 201), (4, 409), (1, 409), (0, 409)],
            5,
        ),
        ("gaps are allowed", &[(1, 201), (7, 201), (1000, 201)], 1000),
        (
            "after a gap, the gap's numbers are gone",
            &[(10, 201), (9, 409), (11, 201), (10, 409)],
            11,
        ),
        (
            "u32::MAX is accepted once",
            &[(u32::MAX, 201), (u32::MAX, 409), (1, 409)],
            u32::MAX,
        ),
        ("first seq may be large", &[(500, 201), (499, 409)], 500),
    ];
    for (i, (name, run, final_seq)) in runs.iter().enumerate() {
        let device = format!("cccccccc-0000-4000-8000-{i:012}");
        for (seq, expected) in run.iter() {
            let resp = s.post(&device, 1000, *seq, [3; 32]).await;
            assert_eq!(resp.status(), *expected, "{name}: seq {seq}");
        }
        assert_eq!(s.state(&device).await["seq"], *final_seq, "{name}");
    }
}

// HASH-006: a 409 changes nothing: the stored hash, seq and last_received stay put.
#[tokio::test]
async fn rejected_seq_leaves_state_unchanged() {
    let s = spawn_server().await;
    assert_eq!(s.post(DEVICE_A, 111, 3, [1; 32]).await.status(), 201);
    let before = s.state(DEVICE_A).await;
    for seq in [3, 2, 0] {
        assert_eq!(s.post(DEVICE_A, 999, seq, [8; 32]).await.status(), 409);
    }
    assert_eq!(s.state(DEVICE_A).await, before);
}

// HASH-006: unix_time is ignored for validation; it is not used for replay prevention.
#[tokio::test]
async fn post_accepts_any_unix_time() {
    let s = spawn_server().await;
    for (seq, unix_time) in [(1u32, 0u32), (2, u32::MAX), (3, 5), (4, 5)] {
        let resp = s.post(DEVICE_A, unix_time, seq, [seq as u8; 32]).await;
        assert_eq!(resp.status(), 201, "seq {seq} time {unix_time}");
    }
}

// HASH-006: stored = sha256(stored || hash), including when the hash repeats and
// when the hash is all zeros.
#[tokio::test]
async fn post_chains_every_hash_including_repeats_and_zeros() {
    let s = spawn_server().await;
    let mut expected = [0u8; 32];
    for (seq, hash) in [
        (1u32, [0u8; 32]),
        (2, [0xff; 32]),
        (3, [0xff; 32]),
        (4, [0; 32]),
    ] {
        assert_eq!(s.post(DEVICE_A, 1, seq, hash).await.status(), 201);
        let next = chain(expected, hash);
        assert_eq!(s.state(DEVICE_A).await["hash"], next, "after seq {seq}");
        expected = hex::decode(next).unwrap().try_into().unwrap();
    }
}

// HASH-006: a 201 response has no error body and the write is visible immediately.
#[tokio::test]
async fn created_response_is_visible_to_the_next_read() {
    let s = spawn_server().await;
    for seq in 1..=10u32 {
        let resp = s.post(DEVICE_A, seq, seq, [seq as u8; 32]).await;
        assert_eq!(resp.status(), 201);
        assert_eq!(s.state(DEVICE_A).await["seq"], seq);
    }
}

// HASH-006: concurrent posts for one device with the same seq: exactly one wins and
// the others get 409, and the chain contains exactly the winner.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_posts_with_the_same_seq_accept_exactly_one() {
    let s = Arc::new(spawn_server().await);
    let mut tasks = Vec::new();
    for i in 0..16u8 {
        let s = s.clone();
        tasks.push(tokio::spawn(async move {
            let resp = s.post(DEVICE_A, 10, 1, [i; 32]).await;
            (i, resp.status().as_u16())
        }));
    }
    let mut winners = Vec::new();
    for t in tasks {
        let (i, status) = t.await.unwrap();
        match status {
            201 => winners.push(i),
            409 => {}
            other => panic!("unexpected status {other}"),
        }
    }
    assert_eq!(winners.len(), 1, "winners: {winners:?}");
    let state = s.state(DEVICE_A).await;
    assert_eq!(state["seq"], 1);
    assert_eq!(state["hash"], chain([0; 32], [winners[0]; 32]));
}

// HASH-006: concurrent posts with distinct seqs are applied in strictly increasing seq
// order, so whatever is accepted ends at the highest accepted seq with a matching chain.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_posts_with_distinct_seqs_never_go_backwards() {
    let s = Arc::new(spawn_server().await);
    let mut tasks = Vec::new();
    for seq in 1..=20u32 {
        let s = s.clone();
        tasks.push(tokio::spawn(async move {
            let resp = s.post(DEVICE_A, 10, seq, [seq as u8; 32]).await;
            (seq, resp.status().as_u16())
        }));
    }
    let mut accepted = Vec::new();
    for t in tasks {
        let (seq, status) = t.await.unwrap();
        assert!(status == 201 || status == 409, "seq {seq}: {status}");
        if status == 201 {
            accepted.push(seq);
        }
    }
    assert!(!accepted.is_empty());
    let state = s.state(DEVICE_A).await;
    assert_eq!(state["seq"], *accepted.iter().max().unwrap());
    // Every accepted seq was applied in increasing order, so the stored hash is the
    // chain over the accepted seqs sorted ascending.
    accepted.sort();
    let mut expected = [0u8; 32];
    for seq in accepted {
        expected = hex::decode(chain(expected, [seq as u8; 32]))
            .unwrap()
            .try_into()
            .unwrap();
    }
    assert_eq!(state["hash"], hex::encode(expected));
}

// HASH-007: one entry per requested device, mixing known and unknown devices.
#[tokio::test]
async fn get_returns_one_entry_per_device() {
    let s = spawn_server().await;
    assert_eq!(s.post(DEVICE_A, 50, 2, [1; 32]).await.status(), 201);
    let unknown = "dddddddd-4444-4444-8444-dddddddddddd";

    let resp = s
        .get_devices(&format!("devices={DEVICE_A},{unknown},{DEVICE_B}"))
        .await;
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    let object = body.as_object().unwrap();
    assert_eq!(object.len(), 3);
    assert_eq!(body[DEVICE_A]["seq"], 2);
    assert_eq!(body[DEVICE_A]["last_received"], 50);
    assert_eq!(body[DEVICE_A]["hash"], chain([0; 32], [1; 32]));
    for id in [unknown, DEVICE_B] {
        assert_eq!(body[id]["hash"], ZERO_HEX);
        assert_eq!(body[id]["seq"], 0);
        assert_eq!(body[id]["last_received"], 0);
    }
    // Each entry has exactly the three documented fields.
    let mut keys: Vec<_> = body[DEVICE_A]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(keys, ["hash", "last_received", "seq"]);
}

// HASH-007: a device is listed once even if requested twice; reading does not create it.
#[tokio::test]
async fn get_with_a_repeated_id_returns_a_single_entry() {
    let s = spawn_server().await;
    let resp = s
        .get_devices(&format!("devices={DEVICE_A},{DEVICE_A}"))
        .await;
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body.as_object().unwrap().len(), 1);
    assert_eq!(body[DEVICE_A]["seq"], 0);
}

// HASH-007: the hash is 64 lowercase hex characters.
#[tokio::test]
async fn get_hash_is_64_hex_characters() {
    let s = spawn_server().await;
    assert_eq!(s.post(DEVICE_A, 1, 1, [0xAB; 32]).await.status(), 201);
    let hash = s.state(DEVICE_A).await["hash"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(hash.len(), 64);
    assert!(
        hash.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
}

// HASH-007: the devices list is validated as a whole: any malformed entry, empty entry
// or missing parameter is a 400 invalid_query and no partial result is returned.
#[tokio::test]
async fn get_rejects_malformed_device_lists() {
    let s = spawn_server().await;
    let cases = [
        ("missing parameter", String::new()),
        ("empty value", "devices=".to_string()),
        ("wrong parameter name", format!("device={DEVICE_A}")),
        ("not a uuid", "devices=abc".to_string()),
        (
            "too short",
            "devices=aaaaaaaa-1111-4111-8111-aaaaaaaaaaa".to_string(),
        ),
        ("too long", format!("devices={DEVICE_A}a")),
        (
            "non-hex character",
            "devices=gggggggg-1111-4111-8111-aaaaaaaaaaaa".to_string(),
        ),
        ("one bad entry", format!("devices={DEVICE_A},nope")),
        ("bad entry first", format!("devices=nope,{DEVICE_A}")),
        (
            "empty entry in the middle",
            format!("devices={DEVICE_A},,{DEVICE_B}"),
        ),
        ("trailing comma", format!("devices={DEVICE_A},")),
        ("leading comma", format!("devices=,{DEVICE_A}")),
        (
            "space separated",
            format!("devices={DEVICE_A}%20{DEVICE_B}"),
        ),
        (
            "semicolon separated",
            format!("devices={DEVICE_A};{DEVICE_B}"),
        ),
    ];
    for (name, query) in cases {
        let resp = s.get_devices(&query).await;
        assert_eq!(resp.status(), 400, "{name}");
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["code"], "invalid_query", "{name}");
    }
}

// HASH-007: the list can be long (a whole account's devices).
#[tokio::test]
async fn get_accepts_a_long_list() {
    let s = spawn_server().await;
    let ids: Vec<String> = (0..200)
        .map(|i| format!("eeeeeeee-0000-4000-8000-{i:012}"))
        .collect();
    let resp = s.get_devices(&format!("devices={}", ids.join(","))).await;
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body.as_object().unwrap().len(), 200);
}

// HASH-008: DELETE of a device the server never saw succeeds with the zero state.
#[tokio::test]
async fn delete_of_an_unknown_device_returns_the_zero_state() {
    let s = spawn_server().await;
    let resp = s.delete_device(&format!("device={DEVICE_B}")).await;
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(
        body,
        json!({ "hash": ZERO_HEX, "seq": 0, "last_received": 0 })
    );
}

// HASH-008: DELETE returns the hash it replaced and leaves last_received alone, seen
// through GET.
#[tokio::test]
async fn delete_returns_prior_hash_and_keeps_last_received() {
    let s = spawn_server().await;
    assert_eq!(s.post(DEVICE_A, 777, 4, [5; 32]).await.status(), 201);
    let before = s.state(DEVICE_A).await;

    let resp = s.delete_device(&format!("device={DEVICE_A}")).await;
    assert_eq!(resp.status(), 200);
    let prior: Value = resp.json().await.unwrap();
    assert_eq!(prior, before);

    let after = s.state(DEVICE_A).await;
    assert_eq!(after["hash"], ZERO_HEX);
    assert_eq!(after["seq"], 0);
    assert_eq!(after["last_received"], 777);
}

// HASH-008: after a reset the chain restarts from zero and seq 1 is valid again.
#[tokio::test]
async fn chain_restarts_from_zero_after_delete() {
    let s = spawn_server().await;
    assert_eq!(s.post(DEVICE_A, 1, 1, [1; 32]).await.status(), 201);
    assert_eq!(s.post(DEVICE_A, 2, 2, [2; 32]).await.status(), 201);
    assert_eq!(
        s.delete_device(&format!("device={DEVICE_A}"))
            .await
            .status(),
        200
    );
    assert_eq!(s.post(DEVICE_A, 3, 1, [3; 32]).await.status(), 201);
    let state = s.state(DEVICE_A).await;
    assert_eq!(state["hash"], chain([0; 32], [3; 32]));
    assert_eq!(state["seq"], 1);
    assert_eq!(state["last_received"], 3);
}

// HASH-008: resetting one device leaves every other device alone.
#[tokio::test]
async fn delete_only_resets_the_named_device() {
    let s = spawn_server().await;
    assert_eq!(s.post(DEVICE_A, 1, 1, [1; 32]).await.status(), 201);
    assert_eq!(s.post(DEVICE_B, 2, 2, [2; 32]).await.status(), 201);
    let b_before = s.state(DEVICE_B).await;
    assert_eq!(
        s.delete_device(&format!("device={DEVICE_A}"))
            .await
            .status(),
        200
    );
    assert_eq!(s.state(DEVICE_B).await, b_before);
}

// HASH-008: a malformed or missing device id is a 400 invalid_query and resets nothing.
#[tokio::test]
async fn delete_rejects_malformed_device_ids() {
    let s = spawn_server().await;
    assert_eq!(s.post(DEVICE_A, 1, 1, [1; 32]).await.status(), 201);
    let cases = [
        ("missing", String::new()),
        ("empty", "device=".to_string()),
        ("not a uuid", "device=device1".to_string()),
        ("two ids", format!("device={DEVICE_A},{DEVICE_B}")),
        ("wrong parameter name", format!("devices={DEVICE_A}")),
        ("trailing junk", format!("device={DEVICE_A}x")),
    ];
    for (name, query) in cases {
        let resp = s.delete_device(&query).await;
        assert_eq!(resp.status(), 400, "{name}");
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["code"], "invalid_query", "{name}");
    }
    assert_eq!(s.state(DEVICE_A).await["seq"], 1);
}

// HASH-009: GET / needs no authentication, even with a bad Authorization header, and
// reports the documented fields.
#[tokio::test]
async fn status_ignores_authorization_and_has_the_documented_shape() {
    let s = spawn_server().await;
    for auth in [None, Some("Bearer garbage"), Some("Basic abc")] {
        let mut req = s.client.get(s.url("/"));
        if let Some(a) = auth {
            req = req.header("Authorization", a);
        }
        let resp = req.send().await.unwrap();
        assert_eq!(resp.status(), 200, "{auth:?}");
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["name"], "Virtue Initiative Hash API");
        assert_eq!(body["status"], "ok");
        let commit = body["commit"].as_str().unwrap();
        assert!(!commit.is_empty());
        let version = body["version"].as_str().unwrap();
        assert!(
            version.split('.').count() == 3 && version.split('.').all(|p| p.parse::<u32>().is_ok()),
            "version {version:?} is not semver-like"
        );
        let mut keys: Vec<_> = body.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["commit", "name", "status", "version"]);
    }
}

// HASH-009: GET / still answers 200 while the server holds data and is under write load.
#[tokio::test]
async fn status_is_ok_after_writes() {
    let s = spawn_server().await;
    assert_eq!(s.post(DEVICE_A, 1, 1, [1; 32]).await.status(), 201);
    let resp = s.client.get(s.url("/")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
}

// HASH-004: the current version prefix works for every endpoint and method, and keeps
// the query string.
#[tokio::test]
async fn current_version_prefix_works_on_every_endpoint() {
    let s = spawn_server().await;
    let device = s.token(DEVICE_A, "device");
    let server = s.token("x", "server");

    let resp = s
        .client
        .post(s.url("/v0.1/hash"))
        .header("Authorization", format!("Bearer {device}"))
        .body(post_body(5, 1, [1; 32]))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let resp = s
        .client
        .get(s.url(&format!("/v0.1/hash?devices={DEVICE_A}")))
        .header("Authorization", format!("Bearer {server}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body[DEVICE_A]["seq"], 1);

    let resp = s
        .client
        .delete(s.url(&format!("/v0.1/hash?device={DEVICE_A}")))
        .header("Authorization", format!("Bearer {server}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(s.state(DEVICE_A).await["seq"], 0);
}

// HASH-004: an unsupported version is 410 on every endpoint and method, before
// authentication, and the request has no effect.
#[tokio::test]
async fn unsupported_versions_are_gone_on_every_endpoint() {
    let s = spawn_server().await;
    for version in ["v0.0", "v0.2", "v0.10", "v1", "v1.0", "v2", "v10"] {
        for (method, path) in [
            (Method::POST, "hash"),
            (Method::GET, "hash?devices=x"),
            (Method::DELETE, "hash?device=x"),
            (Method::GET, ""),
        ] {
            let resp = s
                .client
                .request(method.clone(), s.url(&format!("/{version}/{path}")))
                .body(post_body(1, 1, [1; 32]))
                .send()
                .await
                .unwrap();
            assert_eq!(resp.status(), 410, "{method} /{version}/{path}");
        }
    }
    assert_eq!(s.state(DEVICE_A).await["seq"], 0);
}

// HASH-004: only a well-formed `vN` or `vN.M` first segment is a version; other first
// segments are not treated as versions.
#[tokio::test]
async fn non_version_first_segments_are_not_versions() {
    let s = spawn_server().await;
    for path in [
        "/v/hash",
        "/version/hash",
        "/vx/hash",
        "/v1.2.3/hash",
        "/v0./hash",
        "/hash/v0.1",
    ] {
        let resp = s.client.get(s.url(path)).send().await.unwrap();
        assert_ne!(resp.status(), 410, "{path}");
    }
}
