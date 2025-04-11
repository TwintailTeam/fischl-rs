use serde::{Deserialize, Serialize};

pub type CodebergRelease = Vec<Root2>;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Root2 {
    pub id: i64,
    pub tag_name: String,
    pub target_commitish: String,
    pub name: String,
    pub body: String,
    pub url: String,
    pub html_url: String,
    pub tarball_url: String,
    pub zipball_url: String,
    pub hide_archive_links: bool,
    pub upload_url: String,
    pub draft: bool,
    pub prerelease: bool,
    pub created_at: String,
    pub published_at: String,
    pub author: Author,
    pub assets: Vec<Asset>,
    pub archive_download_count: ArchiveDownloadCount,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Author {
    pub id: i64,
    pub login: String,
    pub login_name: String,
    pub source_id: i64,
    pub full_name: String,
    pub email: String,
    pub avatar_url: String,
    pub html_url: String,
    pub language: String,
    pub is_admin: bool,
    pub last_login: String,
    pub created: String,
    pub restricted: bool,
    pub active: bool,
    pub prohibit_login: bool,
    pub location: String,
    pub pronouns: String,
    pub website: String,
    pub description: String,
    pub visibility: String,
    pub followers_count: i64,
    pub following_count: i64,
    pub starred_repos_count: i64,
    pub username: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub id: i64,
    pub name: String,
    pub size: i64,
    pub download_count: i64,
    pub created_at: String,
    pub uuid: String,
    pub browser_download_url: String,
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchiveDownloadCount {
    pub zip: i64,
    pub tar_gz: i64,
}
