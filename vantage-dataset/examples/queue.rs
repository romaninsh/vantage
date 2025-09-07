// examples/queue.rs

mod mocks;
use mocks::queue_mock::{MockQueue, Topic};

use serde::{Deserialize, Serialize};
use vantage_dataset::dataset::InsertableDataSet;

// This is example implementation of a Queue with multiple topics
// using vantage-dataset pattern. Queue integration is in mocks/, but
// developer would only need to define types to operate with.

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Signup {
    email: String,
    password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResetPassword {
    email: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let queue = MockQueue::init();

    // Developers can now use type safety, when inserting
    // records into topics. You can also Box topic into
    // Box<dyn Insertable>

    let new_signup = Topic::<Signup>::new(&queue);
    let reset_password = Topic::<ResetPassword>::new(&queue);

    // Insert some messages into different topics
    new_signup
        .insert(Signup {
            email: "john".to_string(),
            password: "secret".to_string(),
        })
        .await?;

    new_signup
        .insert(Signup {
            email: "jane".to_string(),
            password: "password123".to_string(),
        })
        .await?;

    reset_password
        .insert(ResetPassword {
            email: "john".to_string(),
        })
        .await?;

    let _b: Box<dyn InsertableDataSet<ResetPassword>> = Box::new(reset_password);

    // Show queue statistics
    println!("Queue Statistics:");
    println!("  Signup messages: {}", queue.message_count("Signup"));
    println!(
        "  Reset password messages: {}",
        queue.message_count("ResetPassword")
    );

    // Show collected messages
    println!("\nAll messages in queue:");
    for (topic, messages) in queue.get_all_messages() {
        println!("Topic '{}': {} messages", topic, messages.len());
        for (i, message) in messages.iter().enumerate() {
            println!("  {}: {}", i + 1, message);
        }
    }

    // Show specific topic messages
    println!("\nSignup topic messages:");
    for message in queue.get_messages("Signup") {
        println!("  {}", message);
    }

    println!("\nReset password topic messages:");
    for message in queue.get_messages("ResetPassword") {
        println!("  {}", message);
    }

    Ok(())
}
