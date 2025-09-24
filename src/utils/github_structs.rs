use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GithubRelease {
    pub url: Option<String>,
    pub assets_url: Option<String>,
    pub upload_url: Option<String>,
    pub html_url: Option<String>,
    pub id: Option<i64>,
    pub author: Author,
    pub node_id: Option<String>,
    pub tag_name: Option<String>,
    pub target_commitish: Option<String>,
    pub name: Option<String>,
    pub draft: Option<bool>,
    pub prerelease: Option<bool>,
    pub created_at: Option<String>,
    pub published_at: Option<String>,
    pub assets: Vec<Asset>,
    pub tarball_url: Option<String>,
    pub zipball_url: Option<String>,
    pub body: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Author {
    pub login: Option<String>,
    pub id: Option<i64>,
    pub node_id: Option<String>,
    pub avatar_url: Option<String>,
    pub gravatar_id: Option<String>,
    pub url: Option<String>,
    pub html_url: Option<String>,
    pub followers_url: Option<String>,
    pub following_url: Option<String>,
    pub gists_url: Option<String>,
    pub starred_url: Option<String>,
    pub subscriptions_url: Option<String>,
    pub organizations_url: Option<String>,
    pub repos_url: Option<String>,
    pub events_url: Option<String>,
    pub received_events_url: Option<String>,
    #[serde(rename = "type")]
    pub type_field: Option<String>,
    pub user_view_type: Option<String>,
    pub site_admin: Option<bool>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub url: Option<String>,
    pub id: Option<i64>,
    pub node_id: Option<String>,
    pub name: Option<String>,
    pub label: Option<String>,
    pub uploader: Uploader,
    pub content_type: Option<String>,
    pub state: Option<String>,
    pub size: Option<i64>,
    pub download_count: Option<i64>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub browser_download_url: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Uploader {
    pub login: Option<String>,
    pub id: Option<i64>,
    pub node_id: Option<String>,
    pub avatar_url: Option<String>,
    pub gravatar_id: Option<String>,
    pub url: Option<String>,
    pub html_url: Option<String>,
    pub followers_url: Option<String>,
    pub following_url: Option<String>,
    pub gists_url: Option<String>,
    pub starred_url: Option<String>,
    pub subscriptions_url: Option<String>,
    pub organizations_url: Option<String>,
    pub repos_url: Option<String>,
    pub events_url: Option<String>,
    pub received_events_url: Option<String>,
    #[serde(rename = "type")]
    pub type_field: Option<String>,
    pub user_view_type: Option<String>,
    pub site_admin: Option<bool>,
}
