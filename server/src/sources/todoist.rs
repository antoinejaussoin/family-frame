//! Todoist REST client for the family to-do column.
//!
//! Personal API token from Todoist → Settings → Integrations → Developer.
//! The server is read-only: it lists open tasks in one shared project.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tracing::info;

use crate::config::TodoistConfig;
use crate::model::TodoItem;
use crate::sources::cache::TtlCache;

use super::contribute::{Contribution, SourceOutcome};
use super::context::SourceContext;
use super::{DataSource, DisabledBehaviour};

pub struct TodoistSource;

#[async_trait::async_trait]
impl DataSource for TodoistSource {
    fn id(&self) -> &'static str {
        "todoist"
    }

    fn enabled(&self, cfg: &crate::config::Config) -> bool {
        cfg.todoist_enabled()
    }

    fn private(&self) -> bool {
        true
    }

    fn when_disabled(&self, _cfg: &crate::config::Config) -> DisabledBehaviour {
        DisabledBehaviour::Demo
    }

    fn disabled_note(&self) -> String {
        "demo to-dos (no Todoist token)".into()
    }

    async fn load(&self, ctx: &SourceContext<'_>) -> Result<SourceOutcome> {
        match load_todos(&ctx.cfg.todoist).await {
            Ok(todos) => Ok(SourceOutcome::live(
                format!("Todoist “{}”", ctx.cfg.todoist.project),
                Contribution::Todos(todos),
            )),
            Err(err) => {
                tracing::warn!(%err, "Todoist failed; using demo to-dos");
                Ok(SourceOutcome::unavailable(
                    "Todoist unavailable",
                    Contribution::Todos(demo_todos()),
                ))
            }
        }
    }

    fn demo(&self, _ctx: &SourceContext<'_>) -> Option<Contribution> {
        Some(Contribution::Todos(demo_todos()))
    }
}

const API: &str = "https://api.todoist.com/api/v1";
const FETCH_TTL: Duration = Duration::from_secs(60);
const PAGE_LIMIT: &str = "200";

static LAST: TtlCache<(String, String, Vec<TodoItem>)> = TtlCache::new();

#[derive(Debug, Deserialize)]
struct Page<T> {
    results: Vec<T>,
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Project {
    #[serde(deserialize_with = "id_as_string")]
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct Task {
    content: String,
    #[serde(default)]
    checked: bool,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    child_order: i64,
}

pub async fn load_todos(cfg: &TodoistConfig) -> Result<Vec<TodoItem>> {
    let token = cfg.token.trim();
    if token.is_empty() {
        bail!("Todoist token is empty");
    }
    let project = cfg.project.trim();
    if let Some((_, _, todos)) =
        LAST.get(FETCH_TTL, |(tok, proj, _)| tok == token && proj == project)
    {
        return Ok(todos);
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .context("Todoist HTTP client")?;
    let project_id = resolve_project(&client, token, project).await?;
    let tasks = fetch_pages::<Task>(
        &client,
        token,
        &format!("{API}/tasks"),
        &[("project_id", project_id.as_str()), ("limit", PAGE_LIMIT)],
    )
    .await
    .context("listing Todoist tasks")?;
    let todos = tasks_to_items(tasks);
    info!(project, n = todos.len(), "loaded Todoist tasks");
    LAST.set((token.to_string(), project.to_string(), todos.clone()));
    Ok(todos)
}

fn tasks_to_items(mut tasks: Vec<Task>) -> Vec<TodoItem> {
    tasks.retain(|t| !t.checked && t.parent_id.as_deref().unwrap_or("").is_empty());
    tasks.sort_by_key(|t| t.child_order);
    tasks
        .into_iter()
        .filter_map(|t| {
            let title = t.content.lines().next().unwrap_or("").trim();
            if title.is_empty() {
                None
            } else {
                Some(TodoItem {
                    title: title.to_string(),
                    done: false,
                })
            }
        })
        .collect()
}

pub fn demo_todos() -> Vec<TodoItem> {
    vec![
        TodoItem {
            title: "Email the school office".into(),
            done: false,
        },
        TodoItem {
            title: "Book MOT".into(),
            done: false,
        },
        TodoItem {
            title: "Return library books".into(),
            done: false,
        },
    ]
}

async fn resolve_project(client: &Client, token: &str, want: &str) -> Result<String> {
    let projects = fetch_pages::<Project>(
        client,
        token,
        &format!("{API}/projects"),
        &[("limit", PAGE_LIMIT)],
    )
    .await
    .context("listing Todoist projects")?;
    if let Some(p) = projects.iter().find(|p| p.id == want) {
        return Ok(p.id.clone());
    }
    if !want.is_empty() {
        if let Some(p) = projects.iter().find(|p| p.name.eq_ignore_ascii_case(want)) {
            return Ok(p.id.clone());
        }
    }
    let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
    if want.is_empty() {
        bail!("Todoist project is empty; available: {}", names.join(", "));
    }
    bail!(
        "Todoist project “{want}” not found; available: {}",
        names.join(", ")
    )
}

async fn fetch_pages<T: DeserializeOwned>(
    client: &Client,
    token: &str,
    url: &str,
    extra: &[(&str, &str)],
) -> Result<Vec<T>> {
    let mut out = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut req = client.get(url).bearer_auth(token);
        for (k, v) in extra {
            req = req.query(&[(k, v)]);
        }
        if let Some(c) = cursor.as_deref() {
            req = req.query(&[("cursor", c)]);
        }
        let page: Page<T> = req
            .send()
            .await?
            .error_for_status()
            .with_context(|| format!("Todoist GET {url}"))?
            .json()
            .await
            .with_context(|| format!("decoding Todoist {url}"))?;
        out.extend(page.results);
        match page.next_cursor {
            Some(next) if !next.is_empty() => cursor = Some(next),
            _ => break,
        }
    }
    Ok(out)
}

fn id_as_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        String(String),
        Number(u64),
    }
    match Id::deserialize(deserializer)? {
        Id::String(s) => Ok(s),
        Id::Number(n) => Ok(n.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_open_root_tasks_in_project_order() {
        let page: Page<Task> = serde_json::from_str(
            r#"{
              "results": [
                {"content": "Later", "checked": false, "parent_id": null, "child_order": 2},
                {"content": "Done", "checked": true, "parent_id": null, "child_order": 0},
                {"content": "Sub", "checked": false, "parent_id": "abc", "child_order": 1},
                {"content": "First\nnotes", "checked": false, "child_order": 1}
              ],
              "next_cursor": null
            }"#,
        )
        .unwrap();
        let items = tasks_to_items(page.results);
        let titles: Vec<&str> = items.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["First", "Later"]);
    }

    #[test]
    fn project_id_accepts_string_or_number() {
        let page: Page<Project> = serde_json::from_str(
            r#"{"results":[{"id":2203306141,"name":"Family"},{"id":"6XGgm","name":"Inbox"}],"next_cursor":null}"#,
        )
        .unwrap();
        assert_eq!(page.results[0].id, "2203306141");
        assert_eq!(page.results[1].id, "6XGgm");
    }

    #[test]
    fn demo_list_is_non_empty() {
        assert!(!demo_todos().is_empty());
    }
}
