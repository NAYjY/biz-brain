//! Quick smoke test: can we reach Gemini and get a valid classification?
//! Usage: cargo run --bin test_gemini

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let api_key = std::env::var("GEMINI_API_KEY")
        .expect("GEMINI_API_KEY not set");

    println!("Using key: {}...{}", &api_key[..8], &api_key[api_key.len()-4..]);

    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "contents": [
            {
                "role": "user",
                "parts": [{ "text": "Classify this message. Reply ONLY with this exact JSON: {\"variant\":\"worker_accepted\",\"order_id\":null}\n\nMessage: รับงานครับ" }]
            }
        ],
        "generationConfig": {
            "maxOutputTokens": 500,
            "temperature": 0.0
        }
    });

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.5-flash-lite:generateContent?key={}",
        api_key
    );

    println!("Sending request to Gemini...");
    let start = std::time::Instant::now();

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await?;

    let elapsed = start.elapsed();
    println!("Response received in {:.2}s", elapsed.as_secs_f32());
    println!("Status: {}", resp.status());

    let json: serde_json::Value = resp.json().await?;
    println!("Full response:\n{}", serde_json::to_string_pretty(&json)?);

    // Try to extract the text
    if let Some(text) = json["candidates"][0]["content"]["parts"][0]["text"].as_str() {
        println!("\nExtracted text: {}", text);
    } else {
        println!("\nWARNING: Could not extract text from response");
    }

    Ok(())
}