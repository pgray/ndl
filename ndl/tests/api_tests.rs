use std::path::PathBuf;

fn get_config_path() -> PathBuf {
    dirs::config_dir().unwrap().join("ndl").join("config.toml")
}

fn load_token() -> Option<String> {
    let path = get_config_path();
    if !path.exists() {
        eprintln!("No config at {:?}, skipping test", path);
        return None;
    }

    let contents = std::fs::read_to_string(&path).ok()?;
    let config: toml::Value = toml::from_str(&contents).ok()?;
    config
        .get("access_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[tokio::test]
async fn test_get_threads() {
    let Some(token) = load_token() else {
        return;
    };

    let client = reqwest::Client::new();
    let url = format!(
        "https://graph.threads.net/me/threads?fields=id,text,username,timestamp,media_type,permalink&limit=5&access_token={}",
        token
    );

    let response = client.get(&url).send().await.unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();

    println!("=== GET /me/threads ===");
    println!("Status: {}", status);
    println!("Response:\n{}", body);

    assert!(status.is_success(), "API call failed: {}", body);
}

#[tokio::test]
async fn test_get_profile() {
    let Some(token) = load_token() else {
        return;
    };

    let client = reqwest::Client::new();
    let url = format!(
        "https://graph.threads.net/me?fields=id,username,name,threads_profile_picture_url,threads_biography&access_token={}",
        token
    );

    let response = client.get(&url).send().await.unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();

    println!("=== GET /me (profile) ===");
    println!("Status: {}", status);
    println!("Response:\n{}", body);

    assert!(status.is_success(), "API call failed: {}", body);
}

#[tokio::test]
async fn test_get_replies() {
    let Some(token) = load_token() else {
        return;
    };

    let client = reqwest::Client::new();
    let url = format!(
        "https://graph.threads.net/me/replies?fields=id,text,username,timestamp&limit=5&access_token={}",
        token
    );

    let response = client.get(&url).send().await.unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();

    println!("=== GET /me/replies ===");
    println!("Status: {}", status);
    println!("Response:\n{}", body);

    assert!(status.is_success(), "API call failed: {}", body);
}

#[tokio::test]
async fn test_get_thread_replies() {
    let Some(token) = load_token() else {
        return;
    };

    // First get a thread ID
    let client = reqwest::Client::new();
    let url = format!(
        "https://graph.threads.net/me/threads?fields=id&limit=1&access_token={}",
        token
    );

    let response = client.get(&url).send().await.unwrap();
    let body = response.text().await.unwrap();
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();

    let Some(thread_id) = json["data"][0]["id"].as_str() else {
        println!("No threads found to test replies");
        return;
    };

    println!("Testing replies for thread: {}", thread_id);

    // Now get replies for that thread
    let url = format!(
        "https://graph.threads.net/{}/replies?fields=id,text,username,timestamp&access_token={}",
        thread_id, token
    );

    let response = client.get(&url).send().await.unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();

    println!("=== GET /{}/replies ===", thread_id);
    println!("Status: {}", status);
    println!("Response:\n{}", body);

    // Note: This might fail if no permission - that's useful info too
}

#[tokio::test]
async fn test_get_nested_replies() {
    let Some(token) = load_token() else {
        return;
    };

    // First get a thread ID
    let client = reqwest::Client::new();
    let url = format!(
        "https://graph.threads.net/me/threads?fields=id,text&limit=10&access_token={}",
        token
    );

    let response = client.get(&url).send().await.unwrap();
    let body = response.text().await.unwrap();
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();

    // Find a TEXT_POST (more likely to have replies)
    let thread_id = json["data"]
        .as_array()
        .and_then(|arr| arr.iter().find(|t| t["text"].is_string()))
        .and_then(|t| t["id"].as_str());

    let Some(thread_id) = thread_id else {
        println!("No text posts found to test nested replies");
        return;
    };

    println!("Testing nested replies for thread: {}", thread_id);

    // Get replies
    let url = format!(
        "https://graph.threads.net/{}/replies?fields=id,text,username&access_token={}",
        thread_id, token
    );

    let response = client.get(&url).send().await.unwrap();
    let body = response.text().await.unwrap();
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();

    println!("=== Level 1 replies ===");
    println!("{}", body);

    // If there are replies, try to get nested replies
    if let Some(replies) = json["data"].as_array() {
        for reply in replies.iter().take(2) {
            if let Some(reply_id) = reply["id"].as_str() {
                let url = format!(
                    "https://graph.threads.net/{}/replies?fields=id,text,username&access_token={}",
                    reply_id, token
                );
                let response = client.get(&url).send().await.unwrap();
                let body = response.text().await.unwrap();
                println!("\n=== Replies to {} ===", reply_id);
                println!("{}", body);
            }
        }
    }
}

#[tokio::test]
async fn test_get_conversation() {
    let Some(token) = load_token() else {
        return;
    };

    // First get a thread ID
    let client = reqwest::Client::new();
    let url = format!(
        "https://graph.threads.net/me/threads?fields=id&limit=1&access_token={}",
        token
    );

    let response = client.get(&url).send().await.unwrap();
    let body = response.text().await.unwrap();
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();

    let Some(thread_id) = json["data"][0]["id"].as_str() else {
        println!("No threads found to test conversation");
        return;
    };

    println!("Testing conversation for thread: {}", thread_id);

    // Try conversation endpoint
    let url = format!(
        "https://graph.threads.net/{}/conversation?fields=id,text,username,timestamp&access_token={}",
        thread_id, token
    );

    let response = client.get(&url).send().await.unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();

    println!("=== GET /{}/conversation ===", thread_id);
    println!("Status: {}", status);
    println!("Response:\n{}", body);
}
