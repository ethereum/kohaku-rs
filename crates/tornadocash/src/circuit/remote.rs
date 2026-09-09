use crate::circuit::Circuit;

const CIRCUIT_URL: &str =
    "https://github.com/Robert-MacWha/privacy-protocol-artifacts/raw/refs/heads/main/artifacts/";

/// Loads a tornadocash circuit from a remote source.
///
/// Requires the `remote-circuit` feature to be enabled.
pub async fn load_remote_circuit() -> Result<Circuit, Box<dyn std::error::Error + Send + Sync>> {
    let circuit_url = format!("{CIRCUIT_URL}/tornadocash-classic/circuit.json.br");
    let pk_url = format!("{CIRCUIT_URL}/tornadocash-classic/proving_key.bin.br");

    let circuit_data = fetch(&circuit_url).await?;
    let pk_data = fetch(&pk_url).await?;

    let circuit_data = decompress(&circuit_data)?;
    let pk_data = decompress(&pk_data)?;

    let circuit: websnark_rs::circuit::Circuit = serde_json::from_slice(&circuit_data)?;
    let pk: websnark_rs::proving_key::ProvingKey = postcard::from_bytes(&pk_data)?;

    Ok(Circuit::new(circuit, pk))
}

async fn fetch(url: &str) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();

    let data = client.get(url).send().await?.bytes().await?.to_vec();
    Ok(data)
}

fn decompress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut out = Vec::new();
    brotli::BrotliDecompress(&mut &data[..], &mut out)?;
    Ok(out)
}
