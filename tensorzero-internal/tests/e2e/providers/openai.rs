#![allow(clippy::print_stdout)]
use std::collections::HashMap;

use reqwest::Client;
use reqwest::StatusCode;
use serde_json::json;
use serde_json::Value;
use tensorzero_internal::{
    embeddings::{EmbeddingProvider, EmbeddingProviderConfig, EmbeddingRequest},
    endpoints::inference::InferenceCredentials,
    inference::types::{Latency, ModelInferenceRequestJsonMode},
};
use uuid::Uuid;

use crate::common::{
    get_clickhouse, get_gateway_endpoint, select_chat_inference_clickhouse,
    select_model_inference_clickhouse,
};
use crate::providers::common::{E2ETestProvider, E2ETestProviders};

#[cfg(feature = "e2e_tests")]
crate::generate_provider_tests!(get_providers);
#[cfg(feature = "batch_tests")]
crate::generate_batch_inference_tests!(get_providers);

async fn get_providers() -> E2ETestProviders {
    let credentials = match std::env::var("OPENAI_API_KEY") {
        Ok(key) => HashMap::from([("openai_api_key".to_string(), key)]),
        Err(_) => HashMap::new(),
    };

    let standard_without_o1 = vec![E2ETestProvider {
        variant_name: "openai".to_string(),
        model_name: "gpt-4o-mini-2024-07-18".into(),
        model_provider_name: "openai".into(),
        credentials: HashMap::new(),
    }];

    let extra_body_providers = vec![E2ETestProvider {
        variant_name: "openai-extra-body".to_string(),
        model_name: "gpt-4o-mini-2024-07-18".into(),
        model_provider_name: "openai".into(),
        credentials: HashMap::new(),
    }];

    let standard_providers = vec![
        E2ETestProvider {
            variant_name: "openai".to_string(),
            model_name: "gpt-4o-mini-2024-07-18".into(),
            model_provider_name: "openai".into(),
            credentials: HashMap::new(),
        },
        E2ETestProvider {
            variant_name: "openai-o1".to_string(),
            model_name: "o1-2024-12-17".into(),
            model_provider_name: "openai".into(),
            credentials: HashMap::new(),
        },
    ];

    let inference_params_providers = vec![E2ETestProvider {
        variant_name: "openai-dynamic".to_string(),
        model_name: "gpt-4o-mini-2024-07-18-dynamic".into(),
        model_provider_name: "openai".into(),
        credentials,
    }];

    let json_providers = vec![
        E2ETestProvider {
            variant_name: "openai".to_string(),
            model_name: "gpt-4o-mini-2024-07-18".into(),
            model_provider_name: "openai".into(),
            credentials: HashMap::new(),
        },
        E2ETestProvider {
            variant_name: "openai-implicit".to_string(),
            model_name: "gpt-4o-mini-2024-07-18".into(),
            model_provider_name: "openai".into(),
            credentials: HashMap::new(),
        },
        E2ETestProvider {
            variant_name: "openai-strict".to_string(),
            model_name: "gpt-4o-mini-2024-07-18".into(),
            model_provider_name: "openai".into(),
            credentials: HashMap::new(),
        },
        E2ETestProvider {
            variant_name: "openai-o1".to_string(),
            model_name: "o1-2024-12-17".into(),
            model_provider_name: "openai".into(),
            credentials: HashMap::new(),
        },
        E2ETestProvider {
            variant_name: "openai-default".to_string(),
            model_name: "gpt-4o-mini-2024-07-18".into(),
            model_provider_name: "openai".into(),
            credentials: HashMap::new(),
        },
    ];

    #[cfg(feature = "e2e_tests")]
    let shorthand_providers = vec![E2ETestProvider {
        variant_name: "openai-shorthand".to_string(),
        model_name: "openai::gpt-4o-mini-2024-07-18".into(),
        model_provider_name: "openai".into(),
        credentials: HashMap::new(),
    }];

    E2ETestProviders {
        simple_inference: standard_providers.clone(),
        extra_body_inference: extra_body_providers,
        reasoning_inference: vec![],
        inference_params_inference: inference_params_providers,
        tool_use_inference: standard_providers.clone(),
        tool_multi_turn_inference: standard_providers.clone(),
        dynamic_tool_use_inference: standard_providers.clone(),
        parallel_tool_use_inference: standard_without_o1.clone(),
        json_mode_inference: json_providers.clone(),
        #[cfg(feature = "e2e_tests")]
        shorthand_inference: shorthand_providers.clone(),
        #[cfg(feature = "batch_tests")]
        supports_batch_inference: true,
    }
}

#[cfg(feature = "e2e_tests")]
#[tokio::test]
pub async fn test_provider_config_extra_body() {
    let episode_id = Uuid::now_v7();

    let payload = json!({
        "function_name": "basic_test",
        "variant_name": "openai-extra-body-provider-config",
        "episode_id": episode_id,
        "params": {
            "chat_completion": {
                "temperature": 9000
            }
        },
        "input":
            {
               "system": {"assistant_name": "Dr. Mehta"},
               "messages": [
                {
                    "role": "user",
                    "content": "What is the name of the capital city of Japan?"
                }
            ]},
        "stream": false,
        "tags": {"foo": "bar"},
    });

    let response = Client::new()
        .post(get_gateway_endpoint("/inference"))
        .json(&payload)
        .send()
        .await
        .unwrap();

    // Check that the API response is ok
    assert_eq!(response.status(), StatusCode::OK);
    let response_json = response.json::<Value>().await.unwrap();

    println!("API response: {response_json:#?}");

    let inference_id = response_json.get("inference_id").unwrap().as_str().unwrap();
    let inference_id = Uuid::parse_str(inference_id).unwrap();

    // Sleep to allow time for data to be inserted into ClickHouse (trailing writes from API)
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Check if ClickHouse is ok - ChatInference Table
    let clickhouse = get_clickhouse().await;

    // Check the ModelInference Table
    let result = select_model_inference_clickhouse(&clickhouse, inference_id)
        .await
        .unwrap();

    println!("ClickHouse - ModelInference: {result:#?}");

    let model_inference_id = result.get("id").unwrap().as_str().unwrap();
    assert!(Uuid::parse_str(model_inference_id).is_ok());

    let inference_id_result = result.get("inference_id").unwrap().as_str().unwrap();
    let inference_id_result = Uuid::parse_str(inference_id_result).unwrap();
    assert_eq!(inference_id_result, inference_id);

    let raw_request = result.get("raw_request").unwrap().as_str().unwrap();
    let raw_request_val: serde_json::Value = serde_json::from_str::<Value>(raw_request).unwrap();

    // This is set in both the variant and model provider extra_body, and
    // so the model provider should win
    assert_eq!(
        raw_request_val
            .get("temperature")
            .unwrap()
            .as_f64()
            .expect("Temperature is not a number"),
        0.456
    );

    // This is only set in the variant extra_body
    assert_eq!(
        raw_request_val
            .get("max_completion_tokens")
            .unwrap()
            .as_u64()
            .expect("max_completion_tokens is not a number"),
        123
    );

    // This is only set in the model provider extra_body
    assert_eq!(
        raw_request_val
            .get("frequency_penalty")
            .unwrap()
            .as_f64()
            .expect("frequency_penalty is not a number"),
        1.42
    );
}

// Tests using 'model_name' with a shorthand model
#[cfg(feature = "e2e_tests")]
#[tokio::test]
async fn test_default_function_model_name_shorthand() {
    let client = Client::new();
    let episode_id = Uuid::now_v7();

    let payload = json!({
        "model_name": "openai::o1-mini",
        "episode_id": episode_id,
        "input": {
            "messages": [
                {
                    "role": "user",
                    "content": "What is the capital of Japan?"
                }
            ]},
        "stream": false,
    });

    let response = client
        .post(get_gateway_endpoint("/inference"))
        .json(&payload)
        .send()
        .await
        .unwrap();
    // Check Response is OK, then fields in order
    assert_eq!(response.status(), StatusCode::OK);
    let response_json = response.json::<Value>().await.unwrap();
    let content_blocks = response_json.get("content").unwrap().as_array().unwrap();
    assert!(content_blocks.len() == 1);
    let content_block = content_blocks.first().unwrap();
    let content_block_type = content_block.get("type").unwrap().as_str().unwrap();
    assert_eq!(content_block_type, "text");
    let content = content_block.get("text").unwrap().as_str().unwrap();
    // Assert that Tokyo is in the content
    assert!(content.contains("Tokyo"), "Content should mention Tokyo");
    // Check that inference_id is here
    let inference_id = response_json.get("inference_id").unwrap().as_str().unwrap();
    let inference_id = Uuid::parse_str(inference_id).unwrap();

    // Sleep for 1 second to allow time for data to be inserted into ClickHouse (trailing writes from API)
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Check ClickHouse
    let clickhouse = get_clickhouse().await;

    // First, check Inference table
    let result = select_chat_inference_clickhouse(&clickhouse, inference_id)
        .await
        .unwrap();
    let id = result.get("id").unwrap().as_str().unwrap();
    let id_uuid = Uuid::parse_str(id).unwrap();
    assert_eq!(id_uuid, inference_id);
    let function_name = result.get("function_name").unwrap().as_str().unwrap();
    assert_eq!(function_name, "tensorzero::default");
    let input: Value =
        serde_json::from_str(result.get("input").unwrap().as_str().unwrap()).unwrap();
    let correct_input = json!({
        "messages": [
            {
                "role": "user",
                "content": [{"type": "text", "value": "What is the capital of Japan?"}]
            }
        ]
    });
    assert_eq!(input, correct_input);
    let content_blocks = result.get("output").unwrap().as_str().unwrap();
    // Check that content_blocks is a list of blocks length 1
    let content_blocks: Vec<Value> = serde_json::from_str(content_blocks).unwrap();
    assert_eq!(content_blocks.len(), 1);
    let content_block = content_blocks.first().unwrap();
    // Check the type and content in the block
    let content_block_type = content_block.get("type").unwrap().as_str().unwrap();
    assert_eq!(content_block_type, "text");
    let clickhouse_content = content_block.get("text").unwrap().as_str().unwrap();
    assert_eq!(clickhouse_content, content);
    // Check that episode_id is here and correct
    let retrieved_episode_id = result.get("episode_id").unwrap().as_str().unwrap();
    let retrieved_episode_id = Uuid::parse_str(retrieved_episode_id).unwrap();
    assert_eq!(retrieved_episode_id, episode_id);
    // Check the variant name
    let variant_name = result.get("variant_name").unwrap().as_str().unwrap();
    assert_eq!(variant_name, "openai::o1-mini");
    // Check the processing time
    let processing_time_ms = result.get("processing_time_ms").unwrap().as_u64().unwrap();
    assert!(processing_time_ms > 0);

    // Check the ModelInference Table
    let result = select_model_inference_clickhouse(&clickhouse, inference_id)
        .await
        .unwrap();
    let inference_id_result = result.get("inference_id").unwrap().as_str().unwrap();
    let inference_id_result = Uuid::parse_str(inference_id_result).unwrap();
    assert_eq!(inference_id_result, inference_id);
    let model_name = result.get("model_name").unwrap().as_str().unwrap();
    assert_eq!(model_name, "openai::o1-mini");
    let model_provider_name = result.get("model_provider_name").unwrap().as_str().unwrap();
    assert_eq!(model_provider_name, "openai");
    let raw_request = result.get("raw_request").unwrap().as_str().unwrap();
    assert!(raw_request.to_lowercase().contains("japan"));
    // Check that raw_request is valid JSON
    let _: Value = serde_json::from_str(raw_request).expect("raw_request should be valid JSON");
    let input_tokens = result.get("input_tokens").unwrap().as_u64().unwrap();
    assert!(input_tokens > 5);
    let output_tokens = result.get("output_tokens").unwrap().as_u64().unwrap();
    assert!(output_tokens > 5);
    let response_time_ms = result.get("response_time_ms").unwrap().as_u64().unwrap();
    assert!(response_time_ms > 0);
    assert!(result.get("ttft_ms").unwrap().is_null());
    let raw_response = result.get("raw_response").unwrap().as_str().unwrap();
    let _raw_response_json: Value = serde_json::from_str(raw_response).unwrap();
}

// Tests using 'model_name' with a non-shorthand model
#[cfg(feature = "e2e_tests")]
#[tokio::test]
async fn test_default_function_model_name_non_shorthand() {
    let client = Client::new();
    let episode_id = Uuid::now_v7();

    let payload = json!({
        "model_name": "o1-mini",
        "episode_id": episode_id,
        "input": {
            "messages": [
                {
                    "role": "user",
                    "content": "What is the capital of Japan?"
                }
            ]},
        "stream": false,
    });

    let response = client
        .post(get_gateway_endpoint("/inference"))
        .json(&payload)
        .send()
        .await
        .unwrap();
    // Check Response is OK, then fields in order
    assert_eq!(response.status(), StatusCode::OK);
    let response_json = response.json::<Value>().await.unwrap();
    let content_blocks = response_json.get("content").unwrap().as_array().unwrap();
    assert!(content_blocks.len() == 1);
    let content_block = content_blocks.first().unwrap();
    let content_block_type = content_block.get("type").unwrap().as_str().unwrap();
    assert_eq!(content_block_type, "text");
    let content = content_block.get("text").unwrap().as_str().unwrap();
    // Assert that Tokyo is in the content
    assert!(content.contains("Tokyo"), "Content should mention Tokyo");
    // Check that inference_id is here
    let inference_id = response_json.get("inference_id").unwrap().as_str().unwrap();
    let inference_id = Uuid::parse_str(inference_id).unwrap();

    // Sleep for 1 second to allow time for data to be inserted into ClickHouse (trailing writes from API)
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Check ClickHouse
    let clickhouse = get_clickhouse().await;

    // First, check Inference table
    let result = select_chat_inference_clickhouse(&clickhouse, inference_id)
        .await
        .unwrap();
    let id = result.get("id").unwrap().as_str().unwrap();
    let id_uuid = Uuid::parse_str(id).unwrap();
    assert_eq!(id_uuid, inference_id);
    let function_name = result.get("function_name").unwrap().as_str().unwrap();
    assert_eq!(function_name, "tensorzero::default");
    let input: Value =
        serde_json::from_str(result.get("input").unwrap().as_str().unwrap()).unwrap();
    let correct_input = json!({
        "messages": [
            {
                "role": "user",
                "content": [{"type": "text", "value": "What is the capital of Japan?"}]
            }
        ]
    });
    assert_eq!(input, correct_input);
    let content_blocks = result.get("output").unwrap().as_str().unwrap();
    // Check that content_blocks is a list of blocks length 1
    let content_blocks: Vec<Value> = serde_json::from_str(content_blocks).unwrap();
    assert_eq!(content_blocks.len(), 1);
    let content_block = content_blocks.first().unwrap();
    // Check the type and content in the block
    let content_block_type = content_block.get("type").unwrap().as_str().unwrap();
    assert_eq!(content_block_type, "text");
    let clickhouse_content = content_block.get("text").unwrap().as_str().unwrap();
    assert_eq!(clickhouse_content, content);
    // Check that episode_id is here and correct
    let retrieved_episode_id = result.get("episode_id").unwrap().as_str().unwrap();
    let retrieved_episode_id = Uuid::parse_str(retrieved_episode_id).unwrap();
    assert_eq!(retrieved_episode_id, episode_id);
    // Check the variant name
    let variant_name = result.get("variant_name").unwrap().as_str().unwrap();
    assert_eq!(variant_name, "o1-mini");
    // Check the processing time
    let processing_time_ms = result.get("processing_time_ms").unwrap().as_u64().unwrap();
    assert!(processing_time_ms > 0);

    // Check the ModelInference Table
    let result = select_model_inference_clickhouse(&clickhouse, inference_id)
        .await
        .unwrap();
    let inference_id_result = result.get("inference_id").unwrap().as_str().unwrap();
    let inference_id_result = Uuid::parse_str(inference_id_result).unwrap();
    assert_eq!(inference_id_result, inference_id);
    let model_name = result.get("model_name").unwrap().as_str().unwrap();
    assert_eq!(model_name, "o1-mini");
    let model_provider_name = result.get("model_provider_name").unwrap().as_str().unwrap();
    assert_eq!(model_provider_name, "openai");
    let raw_request = result.get("raw_request").unwrap().as_str().unwrap();
    assert!(raw_request.to_lowercase().contains("japan"));
    // Check that raw_request is valid JSON
    let _: Value = serde_json::from_str(raw_request).expect("raw_request should be valid JSON");
    let input_tokens = result.get("input_tokens").unwrap().as_u64().unwrap();
    assert!(input_tokens > 5);
    let output_tokens = result.get("output_tokens").unwrap().as_u64().unwrap();
    assert!(output_tokens > 5);
    let response_time_ms = result.get("response_time_ms").unwrap().as_u64().unwrap();
    assert!(response_time_ms > 0);
    assert!(result.get("ttft_ms").unwrap().is_null());
    let raw_response = result.get("raw_response").unwrap().as_str().unwrap();
    let _raw_response_json: Value = serde_json::from_str(raw_response).unwrap();
}

// Tests using 'model_name' with a non-shorthand model
#[cfg(feature = "e2e_tests")]
#[tokio::test]
async fn test_default_function_invalid_model_name() {
    use reqwest::StatusCode;

    let client = Client::new();
    let episode_id = Uuid::now_v7();

    let payload = json!({
        "model_name": "openai::my-bad-model-name",
        "episode_id": episode_id,
        "input": {
            "messages": [
                {
                    "role": "user",
                    "content": "What is the capital of Japan?"
                }
            ]},
        "stream": false,
    });

    let response = client
        .post(get_gateway_endpoint("/inference"))
        .json(&payload)
        .send()
        .await
        .unwrap();
    // Check Response is OK, then fields in order
    let status = response.status();
    let text = response.text().await.unwrap();
    assert!(
        text.contains("`my-bad-model-name` does not exist"),
        "Unexpected error: {text}"
    );
    assert_eq!(status, StatusCode::BAD_GATEWAY);
}

#[cfg(feature = "e2e_tests")]
#[tokio::test]
async fn test_chat_function_json_override_with_mode_on() {
    test_chat_function_json_override_with_mode(ModelInferenceRequestJsonMode::On).await;
}

#[cfg(feature = "e2e_tests")]
#[tokio::test]
async fn test_chat_function_json_override_with_mode_off() {
    test_chat_function_json_override_with_mode(ModelInferenceRequestJsonMode::Off).await;
}

#[cfg(feature = "e2e_tests")]
#[tokio::test]
async fn test_chat_function_json_override_with_mode_strict() {
    test_chat_function_json_override_with_mode(ModelInferenceRequestJsonMode::Strict).await;
}

#[cfg(feature = "e2e_tests")]
#[tokio::test]
async fn test_chat_function_json_override_with_mode_implicit_tool() {
    let client = Client::new();
    let episode_id = Uuid::now_v7();

    // Note that we need to include 'json' somewhere in the messages, to stop OpenAI from complaining
    let payload = json!({
        "function_name": "basic_test",
        "variant_name": "openai",
        "episode_id": episode_id,
        "input":
            {"system": {"assistant_name": "AskJeeves"},
            "messages": [
                {
                    "role": "user",
                    "content": "What is the capital of Japan (possibly as JSON)?"
                }
            ]},
        "params": {
            "chat_completion": {
                "json_mode": "implicit_tool",
            }
        },
        "stream": false,
    });

    let response = client
        .post(get_gateway_endpoint("/inference"))
        .json(&payload)
        .send()
        .await
        .unwrap();
    let response_status = response.status();
    let response_json = response.json::<Value>().await.unwrap();
    // Check Response is OK, then fields in order
    assert_eq!(
        response_status,
        StatusCode::BAD_REQUEST,
        "Unexpected response status, body: {response_json:?})"
    );
    assert_eq!(
        response_json,
        serde_json::json!({
            "error": "JSON mode `implicit_tool` is not supported for chat functions"
        })
    );
}

#[cfg_attr(feature = "batch_tests", allow(unused))]
async fn test_chat_function_json_override_with_mode(json_mode: ModelInferenceRequestJsonMode) {
    let client = Client::new();
    let episode_id = Uuid::now_v7();
    let mode = serde_json::to_value(json_mode).unwrap();

    // Note that we need to include 'json' somewhere in the messages, to stop OpenAI from complaining
    let payload = json!({
        "function_name": "basic_test",
        "variant_name": "openai",
        "episode_id": episode_id,
        "input":
            {"system": {"assistant_name": "AskJeeves"},
            "messages": [
                {
                    "role": "user",
                    "content": "What is the capital of Japan (possibly as JSON)?"
                }
            ]},
        "params": {
            "chat_completion": {
                "json_mode": mode,
            }
        },
        "stream": false,
    });

    let response = client
        .post(get_gateway_endpoint("/inference"))
        .json(&payload)
        .send()
        .await
        .unwrap();
    let response_status = response.status();
    let response_json = response.json::<Value>().await.unwrap();
    // Check Response is OK, then fields in order
    assert_eq!(
        response_status,
        StatusCode::OK,
        "Unexpected response status, body: {response_json:?})"
    );
    let content_blocks = response_json.get("content").unwrap().as_array().unwrap();
    assert!(content_blocks.len() == 1);
    let content_block = content_blocks.first().unwrap();
    let content_block_type = content_block.get("type").unwrap().as_str().unwrap();
    // We should have forwarded the JSON mode to the provider, so OpenAI should give us back json
    // Since we're using a text function, tensorzero should not handle the JSON parsing for us.
    assert_eq!(content_block_type, "text");
    let content = content_block.get("text").unwrap().as_str().unwrap();
    // Assert that Tokyo is in the content
    assert!(content.contains("Tokyo"), "Content should mention Tokyo");
    if let ModelInferenceRequestJsonMode::On | ModelInferenceRequestJsonMode::Strict = json_mode {
        let _context_as_json: Value =
            serde_json::from_str(content).expect("Content should be valid JSON");
    }
    // Check that inference_id is here
    let inference_id = response_json.get("inference_id").unwrap().as_str().unwrap();
    let inference_id = Uuid::parse_str(inference_id).unwrap();

    // Sleep for 1 second to allow time for data to be inserted into ClickHouse (trailing writes from API)
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Check ClickHouse
    let clickhouse = get_clickhouse().await;

    // First, check Inference table
    let result = select_chat_inference_clickhouse(&clickhouse, inference_id)
        .await
        .unwrap();
    let id = result.get("id").unwrap().as_str().unwrap();
    let id_uuid = Uuid::parse_str(id).unwrap();
    assert_eq!(id_uuid, inference_id);
    let input: Value =
        serde_json::from_str(result.get("input").unwrap().as_str().unwrap()).unwrap();
    let correct_input = json!({
        "system": {"assistant_name": "AskJeeves"},
        "messages": [
            {
                "role": "user",
                "content": [{"type": "text", "value": "What is the capital of Japan (possibly as JSON)?"}]
            }
        ]
    });
    assert_eq!(input, correct_input);
    let content_blocks = result.get("output").unwrap().as_str().unwrap();
    // Check that content_blocks is a list of blocks length 1
    let content_blocks: Vec<Value> = serde_json::from_str(content_blocks).unwrap();
    assert_eq!(content_blocks.len(), 1);
    let content_block = content_blocks.first().unwrap();
    // Check the type and content in the block
    let content_block_type = content_block.get("type").unwrap().as_str().unwrap();
    assert_eq!(content_block_type, "text");
    let clickhouse_content = content_block.get("text").unwrap().as_str().unwrap();
    assert_eq!(clickhouse_content, content);
    // Check that episode_id is here and correct
    let retrieved_episode_id = result.get("episode_id").unwrap().as_str().unwrap();
    let retrieved_episode_id = Uuid::parse_str(retrieved_episode_id).unwrap();
    assert_eq!(retrieved_episode_id, episode_id);
    // Check the variant name
    let variant_name = result.get("variant_name").unwrap().as_str().unwrap();
    assert_eq!(variant_name, "openai");
    // Check the processing time
    let processing_time_ms = result.get("processing_time_ms").unwrap().as_u64().unwrap();
    assert!(processing_time_ms > 0);

    // Check that we saved the correct json mode to ClickHouse
    let inference_params = result.get("inference_params").unwrap().as_str().unwrap();
    let inference_params: Value = serde_json::from_str(inference_params).unwrap();
    let expected_json_mode = serde_json::to_value(json_mode).unwrap();
    let clickhouse_json_mode = inference_params
        .get("chat_completion")
        .unwrap()
        .get("json_mode")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(expected_json_mode, clickhouse_json_mode);

    // Check the ModelInference Table
    let result = select_model_inference_clickhouse(&clickhouse, inference_id)
        .await
        .unwrap();
    let inference_id_result = result.get("inference_id").unwrap().as_str().unwrap();
    let inference_id_result = Uuid::parse_str(inference_id_result).unwrap();
    assert_eq!(inference_id_result, inference_id);
    let model_name = result.get("model_name").unwrap().as_str().unwrap();
    assert_eq!(model_name, "gpt-4o-mini-2024-07-18");
    let model_provider_name = result.get("model_provider_name").unwrap().as_str().unwrap();
    assert_eq!(model_provider_name, "openai");
    let raw_request = result.get("raw_request").unwrap().as_str().unwrap();
    assert!(raw_request.to_lowercase().contains("japan"));
    // Check that raw_request is valid JSON
    let raw_request_val: Value =
        serde_json::from_str(raw_request).expect("raw_request should be valid JSON");
    let expected_request = match json_mode {
        ModelInferenceRequestJsonMode::Off => {
            json!({"messages":[{"role":"system","content":"You are a helpful and friendly assistant named AskJeeves"},{"role":"user","content":"What is the capital of Japan (possibly as JSON)?"}],"model":"gpt-4o-mini-2024-07-18","max_completion_tokens":100,"stream":false,"response_format":{"type":"text"}})
        }
        ModelInferenceRequestJsonMode::On => {
            json!({"messages":[{"role":"system","content":"You are a helpful and friendly assistant named AskJeeves"},{"role":"user","content":"What is the capital of Japan (possibly as JSON)?"}],"model":"gpt-4o-mini-2024-07-18","max_completion_tokens":100,"stream":false,"response_format":{"type":"json_object"}})
        }
        ModelInferenceRequestJsonMode::Strict => {
            json!({"messages":[{"role":"system","content":"You are a helpful and friendly assistant named AskJeeves"},{"role":"user","content":"What is the capital of Japan (possibly as JSON)?"}],"model":"gpt-4o-mini-2024-07-18","max_completion_tokens":100,"stream":false,"response_format":{"type":"json_object"}})
        }
    };
    assert_eq!(raw_request_val, expected_request);
    let input_tokens = result.get("input_tokens").unwrap().as_u64().unwrap();
    assert!(input_tokens > 5);
    let output_tokens = result.get("output_tokens").unwrap().as_u64().unwrap();
    assert!(output_tokens > 5);
    let response_time_ms = result.get("response_time_ms").unwrap().as_u64().unwrap();
    assert!(response_time_ms > 0);
    assert!(result.get("ttft_ms").unwrap().is_null());
    let raw_response = result.get("raw_response").unwrap().as_str().unwrap();
    let _raw_response_json: Value = serde_json::from_str(raw_response).unwrap();
}

#[cfg(feature = "e2e_tests")]
#[tokio::test]
async fn test_o1_mini_inference() {
    let client = Client::new();
    let episode_id = Uuid::now_v7();

    let payload = json!({
        "function_name": "basic_test",
        "variant_name": "o1-mini",
        "episode_id": episode_id,
        "input":
            {"system": {"assistant_name": "AskJeeves"},
            "messages": [
                {
                    "role": "user",
                    "content": "What is the capital of Japan?"
                }
            ]},
        "stream": false,
    });

    let response = client
        .post(get_gateway_endpoint("/inference"))
        .json(&payload)
        .send()
        .await
        .unwrap();
    // Check Response is OK, then fields in order
    assert_eq!(response.status(), StatusCode::OK);
    let response_json = response.json::<Value>().await.unwrap();
    let content_blocks = response_json.get("content").unwrap().as_array().unwrap();
    assert!(content_blocks.len() == 1);
    let content_block = content_blocks.first().unwrap();
    let content_block_type = content_block.get("type").unwrap().as_str().unwrap();
    assert_eq!(content_block_type, "text");
    let content = content_block.get("text").unwrap().as_str().unwrap();
    // Assert that Tokyo is in the content
    assert!(content.contains("Tokyo"), "Content should mention Tokyo");
    // Check that inference_id is here
    let inference_id = response_json.get("inference_id").unwrap().as_str().unwrap();
    let inference_id = Uuid::parse_str(inference_id).unwrap();

    // Sleep for 1 second to allow time for data to be inserted into ClickHouse (trailing writes from API)
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Check ClickHouse
    let clickhouse = get_clickhouse().await;

    // First, check Inference table
    let result = select_chat_inference_clickhouse(&clickhouse, inference_id)
        .await
        .unwrap();
    let id = result.get("id").unwrap().as_str().unwrap();
    let id_uuid = Uuid::parse_str(id).unwrap();
    assert_eq!(id_uuid, inference_id);
    let input: Value =
        serde_json::from_str(result.get("input").unwrap().as_str().unwrap()).unwrap();
    let correct_input = json!({
        "system": {"assistant_name": "AskJeeves"},
        "messages": [
            {
                "role": "user",
                "content": [{"type": "text", "value": "What is the capital of Japan?"}]
            }
        ]
    });
    assert_eq!(input, correct_input);
    let content_blocks = result.get("output").unwrap().as_str().unwrap();
    // Check that content_blocks is a list of blocks length 1
    let content_blocks: Vec<Value> = serde_json::from_str(content_blocks).unwrap();
    assert_eq!(content_blocks.len(), 1);
    let content_block = content_blocks.first().unwrap();
    // Check the type and content in the block
    let content_block_type = content_block.get("type").unwrap().as_str().unwrap();
    assert_eq!(content_block_type, "text");
    let clickhouse_content = content_block.get("text").unwrap().as_str().unwrap();
    assert_eq!(clickhouse_content, content);
    // Check that episode_id is here and correct
    let retrieved_episode_id = result.get("episode_id").unwrap().as_str().unwrap();
    let retrieved_episode_id = Uuid::parse_str(retrieved_episode_id).unwrap();
    assert_eq!(retrieved_episode_id, episode_id);
    // Check the variant name
    let variant_name = result.get("variant_name").unwrap().as_str().unwrap();
    assert_eq!(variant_name, "o1-mini");
    // Check the processing time
    let processing_time_ms = result.get("processing_time_ms").unwrap().as_u64().unwrap();
    assert!(processing_time_ms > 0);

    // Check the ModelInference Table
    let result = select_model_inference_clickhouse(&clickhouse, inference_id)
        .await
        .unwrap();
    let inference_id_result = result.get("inference_id").unwrap().as_str().unwrap();
    let inference_id_result = Uuid::parse_str(inference_id_result).unwrap();
    assert_eq!(inference_id_result, inference_id);
    let model_name = result.get("model_name").unwrap().as_str().unwrap();
    assert_eq!(model_name, "o1-mini");
    let model_provider_name = result.get("model_provider_name").unwrap().as_str().unwrap();
    assert_eq!(model_provider_name, "openai");
    let raw_request = result.get("raw_request").unwrap().as_str().unwrap();
    assert!(raw_request.to_lowercase().contains("japan"));
    // Check that raw_request is valid JSON
    let _: Value = serde_json::from_str(raw_request).expect("raw_request should be valid JSON");
    let input_tokens = result.get("input_tokens").unwrap().as_u64().unwrap();
    assert!(input_tokens > 5);
    let output_tokens = result.get("output_tokens").unwrap().as_u64().unwrap();
    assert!(output_tokens > 5);
    let response_time_ms = result.get("response_time_ms").unwrap().as_u64().unwrap();
    assert!(response_time_ms > 0);
    assert!(result.get("ttft_ms").unwrap().is_null());
    let raw_response = result.get("raw_response").unwrap().as_str().unwrap();
    let _raw_response_json: Value = serde_json::from_str(raw_response).unwrap();
}

#[tokio::test]
async fn test_embedding_request() {
    let provider_config_serialized = r#"
    type = "openai"
    model_name = "text-embedding-3-small"
    "#;
    let provider_config: EmbeddingProviderConfig = toml::from_str(provider_config_serialized)
        .expect("Failed to deserialize EmbeddingProviderConfig");
    assert!(matches!(
        provider_config,
        EmbeddingProviderConfig::OpenAI(_)
    ));

    let client = Client::new();
    let request = EmbeddingRequest {
        input: "This is a test input".to_string(),
    };
    let api_keys = InferenceCredentials::default();
    let response = provider_config
        .embed(&request, &client, &api_keys)
        .await
        .unwrap();
    assert_eq!(response.embedding.len(), 1536);
    // Calculate the L2 norm of the embedding
    let norm: f32 = response
        .embedding
        .iter()
        .map(|&x| x.powi(2))
        .sum::<f32>()
        .sqrt();

    // Assert that the norm is approximately 1 (allowing for small floating-point errors)
    assert!(
        (norm - 1.0).abs() < 1e-6,
        "The L2 norm of the embedding should be 1, but it is {}",
        norm
    );
    // Check that the timestamp in created is within 1 second of the current time
    let created = response.created;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as i64;
    assert!(
        (created as i64 - now).abs() <= 1,
        "The created timestamp should be within 1 second of the current time, but it is {}",
        created
    );
    let parsed_raw_response: Value = serde_json::from_str(&response.raw_response).unwrap();
    assert!(
        !parsed_raw_response.is_null(),
        "Parsed raw response should not be null"
    );
    let parsed_raw_request: Value = serde_json::from_str(&response.raw_request).unwrap();
    assert!(
        !parsed_raw_request.is_null(),
        "Parsed raw request should not be null"
    );
    // Hardcoded since the input is 5 tokens
    assert_eq!(response.usage.input_tokens, 5);
    assert_eq!(response.usage.output_tokens, 0);
    match response.latency {
        Latency::NonStreaming { response_time } => {
            assert!(response_time.as_millis() > 100);
        }
        _ => panic!("Latency should be non-streaming"),
    }
}

#[cfg(feature = "e2e_tests")]
#[tokio::test]
async fn test_embedding_sanity_check() {
    let provider_config_serialized = r#"
    type = "openai"
    model_name = "text-embedding-3-small"
    "#;
    let provider_config: EmbeddingProviderConfig = toml::from_str(provider_config_serialized)
        .expect("Failed to deserialize EmbeddingProviderConfig");
    let client = Client::new();
    let embedding_request_a = EmbeddingRequest {
        input: "Joe Biden is the president of the United States".to_string(),
    };

    let embedding_request_b = EmbeddingRequest {
        input: "Kamala Harris is the vice president of the United States".to_string(),
    };

    let embedding_request_c = EmbeddingRequest {
        input: "My favorite systems programming language is Rust".to_string(),
    };
    let api_keys = InferenceCredentials::default();

    // Compute all 3 embeddings concurrently
    let (response_a, response_b, response_c) = tokio::join!(
        provider_config.embed(&embedding_request_a, &client, &api_keys),
        provider_config.embed(&embedding_request_b, &client, &api_keys),
        provider_config.embed(&embedding_request_c, &client, &api_keys)
    );

    // Unwrap the results
    let response_a = response_a.expect("Failed to get embedding for request A");
    let response_b = response_b.expect("Failed to get embedding for request B");
    let response_c = response_c.expect("Failed to get embedding for request C");

    // Calculate cosine similarities
    let similarity_ab = cosine_similarity(&response_a.embedding, &response_b.embedding);
    let similarity_ac = cosine_similarity(&response_a.embedding, &response_c.embedding);
    let similarity_bc = cosine_similarity(&response_b.embedding, &response_c.embedding);

    // Assert that semantically similar sentences have higher similarity (with a margin of 0.3)
    // We empirically determined this by staring at it (no science to it)
    assert!(
        similarity_ab - similarity_ac > 0.3,
        "Similarity between A and B should be higher than between A and C"
    );
    assert!(
        similarity_ab - similarity_bc > 0.3,
        "Similarity between A and B should be higher than between B and C"
    );
}

#[cfg(feature = "e2e_tests")]
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot_product / (magnitude_a * magnitude_b)
}
