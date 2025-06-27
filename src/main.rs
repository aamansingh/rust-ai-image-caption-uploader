use actix_multipart::Multipart;
use actix_web::{post, web, App, HttpResponse, HttpServer, Responder};
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use dotenv::dotenv;
use futures_util::StreamExt;
use std::env;
use uuid::Uuid;
use serde::Deserialize;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};

#[derive(Debug, Deserialize)]
struct CaptionResponse {
    generated_text: String,
}

pub async fn get_image_caption(image_url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let hf_token = env::var("HF_TOKEN")?;
    let client = reqwest::Client::new();

    let models = vec![
        "Salesforce/blip-image-captioning-large",
        "Salesforce/blip-image-captioning-base",
        "nlpconnect/vit-gpt2-image-captioning"
    ];

    for model in models {
        println!("🧪 Trying model: {}", model);

        let body = serde_json::json!({
            "inputs": image_url
        });

        let res = client
            .post(&format!("https://api-inference.huggingface.co/models/{}", model))
            .header(AUTHORIZATION, format!("Bearer {}", hf_token))
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;

        if !res.status().is_success() {
            println!("❌ Model {} returned HTTP {}", model, res.status());
            continue;
        }

        let text = res.text().await?;
        println!("🔍 Hugging Face raw response:\n{}", text);

        match serde_json::from_str::<Vec<CaptionResponse>>(&text) {
            Ok(caption_json) if !caption_json.is_empty() => {
                return Ok(caption_json[0].generated_text.clone());
            }
            Err(e) => {
                println!("⚠️ Model {} failed to parse. Trying next...", model);
                println!("🔴 Parse error: {}", e);
            }
            _ => continue,
        }
    }

    Err("All models failed to generate a valid caption.".into())
}


#[post("/upload")]
async fn upload_image(mut payload: Multipart) -> impl Responder {
    dotenv().ok();

    let bucket_name = match env::var("S3_BUCKET_NAME") {
        Ok(val) => val,
        Err(_) => return HttpResponse::InternalServerError().body("Server error: missing bucket name."),
    };

    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let s3_client = Client::new(&config);
    println!("📥 Received upload request.");
    println!("✅ AWS S3 client initialized.");

    while let Some(field_result) = payload.next().await {
        if let Ok(mut field) = field_result {
            println!("📦 Processing next field...");

            let filename = format!("{}.jpg", Uuid::new_v4());
            println!("📝 New filename: {}", filename);

            let mut data = web::BytesMut::new();
            while let Some(chunk) = field.next().await {
                match chunk {
                    Ok(bytes) => data.extend_from_slice(&bytes),
                    Err(_) => return HttpResponse::BadRequest().body("Error reading file chunk."),
                }
            }

            let byte_stream = ByteStream::from(data.to_vec());
            println!("⬆️ Uploading {} to S3...", filename);

            let resp = s3_client
                .put_object()
                .bucket(&bucket_name)
                .key(&filename)
                .body(byte_stream)
                .send()
                .await;

            match resp {
                Ok(_) => {
                    println!("✅ Upload successful: {}", filename);

                    let image_url = format!(
                        "https://{}.s3.ap-south-1.amazonaws.com/{}",
                        bucket_name, filename
                    );
                    println!("🔗 Uploaded image URL: {}", image_url);

                    match get_image_caption(&image_url).await {
                        Ok(caption) => {
                            println!("🧠 Caption: {}", caption);
                            return HttpResponse::Ok()
                                .body(format!("✅ Uploaded + Caption: {}\n🧠 {}", filename, caption));
                        }
                        Err(e) => {
                            println!("❌ Failed to generate caption: {:?}", e);
                            return HttpResponse::Ok()
                                .body(format!("✅ Uploaded: {}, but caption failed.", filename));
                        }
                    }
                }
                Err(_) => return HttpResponse::InternalServerError().body("Upload to S3 failed."),
            }
        }
    }
    
    HttpResponse::BadRequest().body("No file uploaded.")
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    HttpServer::new(|| App::new().service(upload_image))
        .bind(("127.0.0.1", 8080))?
        .run()
        .await
}

// ✅ proper unit test module
#[cfg(test)]
mod tests {
    #[test]
    fn hello() {
        println!("✅ Running test: hello");
        assert_eq!(2 + 2, 4);
    }
}
