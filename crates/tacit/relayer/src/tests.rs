//! The client against a local stand-in relayer: canned answers, no network.

use std::sync::{Arc, Mutex};

use alloy::primitives::address;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use super::*;

type Route = (u16, String);

/// Serves `routes` (path prefix → status and body) and records each request line.
async fn serve(routes: Vec<(&'static str, Route)>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/evm-pool/keeper", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut buf = vec![0u8; 65536];
            let n = socket.read(&mut buf).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = request.lines().next().unwrap_or_default().to_string();
            log.lock().unwrap().push(request.clone());
            let path = line.split(' ').nth(1).unwrap_or_default();
            let (status, body) = routes
                .iter()
                .find(|(prefix, _)| path.starts_with(&format!("/evm-pool/keeper{prefix}")))
                .map_or((404, "{}".to_string()), |(_, r)| r.clone());
            let reply = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });
    (url, seen)
}

const RELAYER: Address = address!("0x7c9f8aE4e48Cbb2727F95b6477a1cf92bCFc43D0");

fn client(url: &str) -> RelayerClient {
    let pool = Pool::ethereum().with_relayer(Some(RelayerConfig {
        url: url.to_string(),
        address: RELAYER,
        max_fee: 1_000_000,
    }));
    RelayerClient::new(&pool).unwrap()
}

fn quote_body(chain: u64, relayer: &str, fee: &str) -> String {
    format!(
        r#"{{"chainId":{chain},"pool":"0x000000c2A20657CE25f2Ba99737933D031AFBEE9","relayer":"{relayer}","asset":"0x0000000000000000000000000000000000000000","fee":"{fee}","gas":"400000"}}"#
    )
}

fn relay_tx() -> RelayTx {
    let s = |x: u32| x.to_string();
    RelayTx {
        pa: [s(1), s(2)],
        pb: [[s(3), s(4)], [s(5), s(6)]],
        pc: [s(7), s(8)],
        public_inputs: (0..11).map(s).collect(),
        recipient: Address::ZERO,
        ext_amount: "0".into(),
        relayer: RELAYER,
        fee: "500".into(),
        memo0: Bytes::new(),
        memo1: Bytes::new(),
    }
}

#[tokio::test]
async fn accepts_a_quote_that_passes_every_check() {
    let relayer = "0x7c9f8ae4e48cbb2727f95b6477a1cf92bcfc43d0";
    let (url, _) = serve(vec![("/quote", (200, quote_body(1, relayer, "500")))]).await;
    let quote = client(&url).quote().await.unwrap();
    assert_eq!(
        quote,
        Quote {
            relayer: RELAYER,
            fee: 500
        }
    );
    assert_eq!(
        quote.payer(),
        Payer {
            relayer: RELAYER,
            fee: 500
        }
    );
}

#[tokio::test]
async fn refuses_quotes_for_another_pool_address_or_price() {
    let ours = "0x7c9f8aE4e48Cbb2727F95b6477a1cf92bCFc43D0";
    for (body, check) in [
        (
            quote_body(8453, ours, "500"),
            (|e: &RelayerError| matches!(e, RelayerError::WrongPool)) as fn(&RelayerError) -> bool,
        ),
        (
            quote_body(1, "0x0000000000000000000000000000000000000001", "500"),
            |e| matches!(e, RelayerError::WrongAddress(_)),
        ),
        (quote_body(1, ours, "1000001"), |e| {
            matches!(
                e,
                RelayerError::FeeAboveLimit {
                    quoted: 1_000_001,
                    limit: 1_000_000
                }
            )
        }),
        (quote_body(1, ours, "1e9"), |e| {
            matches!(e, RelayerError::Malformed(_))
        }),
    ] {
        let (url, _) = serve(vec![("/quote", (200, body))]).await;
        let error = client(&url).quote().await.unwrap_err();
        assert!(check(&error), "{error}");
    }
}

#[tokio::test]
async fn reads_the_queued_head() {
    let body = r#"{"root":"5","size":"52","pending":[{"outLeaf0":"7","outLeaf1":"0x08","newRoot":"9"}],"tail":{"root":"9","size":"54"}}"#;
    let (url, _) = serve(vec![("/head", (200, body.to_string()))]).await;
    let head = client(&url).head().await.unwrap();
    assert_eq!(
        head,
        Head {
            root: U256::from(5),
            size: 52,
            pending: vec![[U256::from(7), U256::from(8)]],
            tail_root: U256::from(9),
            tail_size: 54,
        }
    );
}

#[tokio::test]
async fn reads_the_feed_and_refuses_another_pools() {
    let page = r#"{"chainId":1,"pool":"0x000000c2A20657CE25f2Ba99737933D031AFBEE9","through":120,"events":[
        {"kind":"received","block":101,"box":"0x00","n":"1","index":0,"value":"1","rho":"1","fee":"0"},
        {"kind":"transact","block":110,"tx":null,"firstIndex":4,"nf0":"0x0000000000000000000000000000000000000000000000000000000000000003","nf1":"0x0000000000000000000000000000000000000000000000000000000000000000","outLeaf0":"11","outLeaf1":"12","memo0":"0xabcd","memo1":"0x"}]}"#;
    let (url, seen) = serve(vec![("/events", (200, page.to_string()))]).await;
    let feed = client(&url).events(100).await.unwrap();
    assert!(seen.lock().unwrap()[0].starts_with("GET /evm-pool/keeper/events?from=100 "));
    assert_eq!(feed.through, 120);
    assert_eq!(feed.events.len(), 1);
    let event = &feed.events[0];
    assert_eq!(event.nullifiers, [U256::from(3), U256::ZERO]);
    assert_eq!(event.out_leaves, [U256::from(11), U256::from(12)]);
    assert_eq!(event.first_index, 4);
    assert_eq!(event.memos[0].as_ref(), &[0xab, 0xcd]);
    assert_eq!(event.block, 110);

    let other = page.replace("\"chainId\":1", "\"chainId\":10");
    let (url, _) = serve(vec![("/events", (200, other))]).await;
    assert!(matches!(
        client(&url).events(100).await,
        Err(RelayerError::WrongPool)
    ));
}

#[tokio::test]
async fn relays_and_names_each_refusal() {
    let hash = "0x1111111111111111111111111111111111111111111111111111111111111111";
    let (url, seen) = serve(vec![("/relay", (200, format!(r#"{{"txHash":"{hash}"}}"#)))]).await;
    let got = client(&url).relay(&relay_tx()).await.unwrap();
    assert_eq!(got, B256::from_str(hash).unwrap());
    let request = seen.lock().unwrap()[0].clone();
    assert!(request.starts_with("POST /evm-pool/keeper/relay "));
    let body: serde_json::Value =
        serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["tx"]["pB"][1][0], "5");
    assert_eq!(body["tx"]["publicInputs"].as_array().unwrap().len(), 11);
    assert_eq!(body["tx"]["extAmount"], "0");

    for (status, body, check) in [
        (
            502,
            String::new(),
            (|e: &RelayerError| matches!(e, RelayerError::NoAnswer)) as fn(&RelayerError) -> bool,
        ),
        (400, r#"{"error":"AlreadyNullified"}"#.into(), |e| {
            matches!(e, RelayerError::AlreadySpent)
        }),
        (
            409,
            r#"{"error":"this spend is already being relayed","stale":true}"#.into(),
            |e| matches!(e, RelayerError::InFlight),
        ),
        (
            409,
            r#"{"error":"UnknownRoot: re-prove against the pool's current root","stale":true}"#
                .into(),
            |e| matches!(e, RelayerError::Stale(_)),
        ),
        (
            400,
            r#"{"error":"fee too low: below cost","needFee":"900"}"#.into(),
            |e| {
                matches!(
                    e,
                    RelayerError::FeeTooLow {
                        needed: 900,
                        offered: 500
                    }
                )
            },
        ),
        (400, r#"{"error":"bad memo"}"#.into(), |e| {
            matches!(e, RelayerError::Refused { status: 400, .. })
        }),
    ] {
        let (url, _) = serve(vec![("/relay", (status, body))]).await;
        let error = client(&url).relay(&relay_tx()).await.unwrap_err();
        assert!(check(&error), "{status}: {error}");
    }
}

#[tokio::test]
async fn a_relayer_that_never_answers_may_still_have_sent() {
    // Nothing listens on a port that was just released.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/evm-pool/keeper", listener.local_addr().unwrap());
    drop(listener);
    assert!(matches!(
        client(&url).relay(&relay_tx()).await,
        Err(RelayerError::NoAnswer)
    ));
}
