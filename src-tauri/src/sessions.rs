use crate::{auth, cloud::{AccountConfig, CloudClient}, store::Store};
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sessions {
    pub active_id: Option<String>,
    pub accounts: Vec<AccountConfig>,
    #[serde(default)]
    pub sources: Vec<BrowseSource>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BrowseSource {
    pub id: String,
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner_id: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub removed: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reference: String,
}

impl BrowseSource {
    pub async fn client(&self, account: &CloudClient) -> Result<CloudClient, String> {
        account.for_browse_source(&self.kind, &self.reference).await
    }
}

impl Sessions {
    pub fn load(store: &Store) -> Result<Self, String> {
        let Some(value) = store.load_account::<serde_json::Value>()? else {
            return Ok(Self::default());
        };
        let legacy = value.get("accounts").is_none();
        let mut vault: Self = if legacy {
            let mut config: AccountConfig =
                serde_json::from_value(value).map_err(|_| "保存的凭据格式无效")?;
            if config.id.is_empty() {
                config.id = uuid::Uuid::new_v4().to_string();
            }
            Self {
                active_id: Some(config.id.clone()),
                accounts: vec![config],
                ..Self::default()
            }
        } else {
            serde_json::from_value(value).map_err(|_| "会话存储格式无效")?
        };
        if vault.migrate() || legacy {
            vault.save(store)?;
        }
        Ok(vault)
    }

    fn migrate(&mut self) -> bool {
        let mut changed = false;
        self.accounts.retain_mut(|config| {
            if config.space == "personal_new" {
                return true;
            }
            changed = true;
            let references = match config.space.as_str() {
                "share" => crate::cloud::saved_share_inputs(&config.link_id),
                "group" if !config.cloud_id.is_empty() => vec![config.cloud_id.clone()],
                _ => Vec::new(),
            };
            let authenticated = !config.authorization.is_empty()
                || !config.mail_cookies.is_empty() || !config.username.is_empty();
            let owner_id = if authenticated { config.id.clone() } else { String::new() };
            for reference in references {
                if !self.sources.iter().any(|s| s.owner_id == owner_id && s.kind == config.space && s.reference == reference) {
                    let name = reference.split('#').next().unwrap_or(&reference);
                    self.sources.push(BrowseSource {
                        id: uuid::Uuid::new_v4().to_string(),
                        kind: config.space.clone(),
                        name: if config.space == "group" { format!("群组 {name}") } else { name.into() },
                        owner_id: owner_id.clone(),
                        removed: false,
                        reference,
                    });
                }
            }
            config.keep_personal_space();
            authenticated
        });
        if !self.accounts.iter().any(|entry| Some(&entry.id) == self.active_id.as_ref()) {
            let active = self.accounts.first().map(|entry| entry.id.clone());
            changed |= self.active_id != active;
            self.active_id = active;
        }
        self.assign_legacy_sources() || changed
    }

    fn assign_legacy_sources(&mut self) -> bool {
        let Some(owner) = self.active_id.as_ref() else { return false; };
        let mut changed = false;
        for source in self.sources.iter_mut().filter(|source| source.owner_id.is_empty()) {
            source.owner_id.clone_from(owner);
            changed = true;
        }
        changed
    }

    pub fn source(&self, owner: &str, id: &str, include_removed: bool) -> Result<&BrowseSource, String> {
        self.sources.iter().find(|source| {
            source.owner_id == owner && source.id == id && (include_removed || !source.removed)
        }).ok_or_else(|| "该浏览来源不存在".into())
    }

    pub fn save(&self, store: &Store) -> Result<(), String> {
        store.save_account(self)
    }

    pub fn upsert(&mut self, mut config: AccountConfig) -> Result<(), String> {
        config.password.clear();
        config.sms_code.clear();
        if let Some(existing) = self.accounts.iter_mut().find(|entry| entry.id == config.id) {
            *existing = config.clone();
        } else {
            if self.accounts.len() >= 64 {
                return Err("最多保存 64 个账号，请先删除不再使用的账号".into());
            }
            self.accounts.push(config.clone());
        }
        self.active_id = Some(config.id);
        self.assign_legacy_sources();
        Ok(())
    }

    pub fn matching_id(&self, config: &AccountConfig) -> Option<String> {
        let key = identity(config);
        if key == "share:" {
            return None;
        }
        self.accounts
            .iter()
            .find(|entry| identity(entry) == key)
            .map(|entry| entry.id.clone())
    }
}

fn identity(config: &AccountConfig) -> String {
    let account = auth::account_from_authorization(&config.authorization)
        .unwrap_or_else(|_| config.username.clone());
    if account.is_empty() {
        format!("share:{}", config.link_id)
    } else {
        account
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_spaces_move_to_sources_without_replacing_personal_account() {
        let mut vault = Sessions {
            active_id: Some("guest".into()),
            accounts: vec![
                AccountConfig {
                    id: "guest".into(), space: "share".into(), root_id: "root".into(),
                    link_id: "one#pass,two".into(), ..Default::default()
                },
                AccountConfig {
                    id: "account".into(), authorization: "saved-auth".into(),
                    space: "group".into(), cloud_id: "group-123".into(),
                    root_id: "group-123".into(), ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert!(vault.migrate());
        assert_eq!(vault.active_id.as_deref(), Some("account"));
        assert_eq!(vault.accounts.len(), 1);
        assert_eq!(vault.accounts[0].authorization, "saved-auth");
        assert_eq!(vault.accounts[0].space, "personal_new");
        assert!(vault.accounts[0].root_id.is_empty());
        assert_eq!(vault.sources.iter().map(|source| source.reference.as_str()).collect::<Vec<_>>(),
            ["one#pass", "two#", "group-123"]);
        assert!(!vault.migrate());
    }
    #[test]
    fn legacy_sources_are_assigned_once_and_removed_sources_are_download_only() {
        let mut vault = Sessions {
            active_id: Some("a".into()),
            accounts: ["a", "b"].into_iter().map(|id| AccountConfig {
                id: id.into(), ..Default::default()
            }).collect(),
            sources: vec![serde_json::from_value(serde_json::json!({
                "id": "saved", "kind": "share", "name": "Saved share", "reference": "link#code"
            })).unwrap()],
        };
        assert!(vault.migrate());
        assert!(vault.source("a", "saved", false).is_ok());
        vault.active_id = Some("b".into());
        assert!(!vault.migrate());
        assert!(vault.source("b", "saved", true).is_err());
        vault.sources[0].removed = true;
        assert!(vault.source("a", "saved", false).is_err());
        assert!(vault.source("a", "saved", true).is_ok());
    }
    #[test]
    fn sessions_preserve_other_credentials_and_clear_secrets() {
        let mut vault = Sessions::default();
        for id in ["a", "b"] {
            vault
                .upsert(AccountConfig {
                    id: id.into(),
                    username: id.into(),
                    password: "secret".into(),
                    sms_code: "123456".into(),
                    ..Default::default()
                })
                .unwrap();
        }
        vault
            .upsert(AccountConfig {
                id: "a".into(),
                name: "updated".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(vault.accounts.len(), 2);
        assert_eq!(vault.active_id.as_deref(), Some("a"));
        assert_eq!(vault.accounts[1].username, "b");
        assert!(vault
            .accounts
            .iter()
            .all(|config| config.password.is_empty() && config.sms_code.is_empty()));
    }
}
