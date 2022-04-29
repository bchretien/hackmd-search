use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use clap::Parser;
use futures::{stream, StreamExt};
use regex::Regex;
use serde::{Deserialize, Serialize};

use thiserror::Error;

const SERVER_URL: &str = "https://hackmd.io";

#[derive(Error, Debug)]
pub enum UserInputError {
    #[error("Missing required argument: --{arg}")]
    MissingArgument { arg: String },
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Name of the HackMD team.
    #[clap(short, long)]
    team: Option<String>,

    /// Path to the output JSON database.
    #[clap(short, long, default_value = "hackmd.json")]
    database: String,

    /// Whether to update the database.
    #[clap(short, long)]
    update: bool,

    /// Meilisearch URL.
    #[clap(short, long)]
    meilisearch: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Page {
    id: String,
    title: String,
    lastchange_at: String,
    content: Option<String>,
}

async fn build_database(team: &str) -> anyhow::Result<Vec<Page>> {
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .tcp_keepalive(Duration::new(60, 0))
        .build()?;

    // Retry up to 3 times with increasing intervals between attempts.
    let retry_policy =
        reqwest_retry::policies::ExponentialBackoff::builder().build_with_max_retries(3);

    let client = reqwest_middleware::ClientBuilder::new(client)
        .with(reqwest_retry::RetryTransientMiddleware::new_with_policy(
            retry_policy,
        ))
        .build();

    // Get CSRF token
    // See: https://hackmd.io/@ystl/BkqNtYvrP
    let response = client.get(SERVER_URL).send().await?;
    let content = response.text().await?;
    let re = Regex::new(r#""csrf-token" content="(.+)""#).unwrap();
    let cap = re
        .captures_iter(&content)
        .next()
        .ok_or_else(|| anyhow::anyhow!("no CSRF token found"))?;
    let csrf_token = String::from(&cap[1]);
    println!("CSRF token: {}", &csrf_token);

    // Login
    let login_url = format!("{server}/login", server = SERVER_URL);
    let mut params = HashMap::new();

    print!("HackMD user: ");
    std::io::stdout().flush()?;
    let mut hackmd_user = String::new();
    std::io::stdin()
        .read_line(&mut hackmd_user)
        .expect("error: unable to read user input");
    let hackmd_pass = rpassword::prompt_password("HackMD password: ")?;

    params.insert("email", hackmd_user.trim());
    params.insert("password", &hackmd_pass);

    let response = client
        .post(&login_url)
        .header("X-XSRF-Token", csrf_token)
        .form(&params)
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("Login failure");
    }

    // Query
    let request_url = format!(
        "{server}/api/overview/team/{team}",
        server = SERVER_URL,
        team = team
    );
    let response = client.get(&request_url).send().await?.error_for_status()?;
    let mut page_list: Vec<Page> = response.json().await?;

    const CONCURRENT_REQUESTS: usize = 5;
    let bodies = stream::iter(&page_list)
        .map(|page| {
            let client = &client;

            println!("Downloading {}", page.id);
            let page_url = format!("{server}/{id}/download", server = SERVER_URL, id = page.id);

            async move {
                let response = client.get(&page_url).send().await?;
                response.text().await.map_err(anyhow::Error::new)
            }
        })
        .buffered(CONCURRENT_REQUESTS);

    bodies
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .enumerate()
        .for_each(|(idx, text)| {
            if let Ok(content) = text {
                page_list[idx].content = Some(content);
            }
        });

    Ok(page_list)
}

async fn to_meilisearch(page_list: &[Page], url: &str) -> anyhow::Result<()> {
    // Add documents to meilisearch
    let search_client = meilisearch_sdk::client::Client::new(url, "masterKey");
    let _health = search_client.health().await?;
    let pages_index = search_client.get_index("pages").await;
    let pages_index = match pages_index {
        Ok(index) => index,
        _ => {
            let task = search_client.create_index("pages", None).await?;
            let task = task.wait_for_completion(&search_client, None, None).await?;
            let result = task.try_make_index(&search_client);
            match result {
                Ok(index) => index,
                Err(task) => {
                    return Err(
                        meilisearch_sdk::errors::Error::Meilisearch(task.unwrap_failure()).into(),
                    )
                }
            }
        }
    };
    pages_index.add_or_replace(page_list, Some("id")).await?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    anyhow::ensure!(
        !args.database.is_empty(),
        UserInputError::MissingArgument {
            arg: "database".to_string()
        }
    );

    let page_list = if args.update || !Path::new(&args.database).is_file() {
        println!("Building HackMD database...");

        anyhow::ensure!(
            args.team.is_some(),
            UserInputError::MissingArgument {
                arg: "team".to_string()
            }
        );

        let page_list = build_database(&args.team.unwrap()).await?;

        println!("Dumping HackMD database to {}", args.database);
        let f = File::create(args.database).expect("Unable to create file");
        let f = BufWriter::new(f);
        serde_json::to_writer(f, &page_list)?;

        page_list
    } else {
        println!("Loading HackMD database from {}", args.database);
        let file = File::open(args.database)?;
        let reader = BufReader::new(file);
        serde_json::from_reader(reader)?
    };

    if args.meilisearch.is_some() {
        to_meilisearch(&page_list, &args.meilisearch.unwrap()).await?;
    }

    Ok(())
}
